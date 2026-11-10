use chrono::Utc;
use clap::Parser;
use image::{DynamicImage, GenericImage, ImageFormat, RgbaImage};
use regex::Regex;
use reqwest::{multipart, Client};
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::{Mutex, Semaphore};
use url::Url;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 AmoDL/0.3";

static AD_IMG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:logo|banner|discord|patreon|donation|avatar|advertisement)\b|/ads/|\bads\b|[._-]ads[._-]").unwrap()
});

static MANGADEX_RE1: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)chapter/([0-9a-f-]{36})").unwrap()
});

static MANGADEX_RE2: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)([0-9a-f-]{36})").unwrap()
});

static VALIR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:\\"|")imageUrl(?:\\"|")\s*:\s*(?:\\"|")([^"\\]+(?:\\.[^"\\]*)*)(?:\\"|")"#).unwrap()
});

static NEXT_SERIES_IMG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:/uploads|/images)/series/[^\s"'\\<>]+?\.(?:webp|jpg|jpeg|png|avif)"#).unwrap()
});

static AJAX_RE_CH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:\\"|")chapter(?:\\"|")\s*:\s*\{[^}]*(?:\\"|")id(?:\\"|")\s*:\s*(?:\\"|")([a-z0-9-]+)(?:\\"|")"#).unwrap()
});

static AJAX_RE_FALLBACK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:chapterId|chapter_id)[=:"']([a-z0-9-]+)"#).unwrap()
});

static LOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:this chapter is locked|chapter is locked|locked chapter|premium chapter|please purchase it to read|unlock to continue reading|sign in to unlock|unlocking grants permanent access|unlock this (?:exciting )?chapter|unlock(?:s)? for \d+ coins?|cost to unlock|fast pass chapter|early access chapter|get coins to unlock|coins to read)").unwrap()
});

static PREM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:-\s*premium\b|\bearly access\b|\[premium\]|\(premium\)|\bpremium chapter\b)").unwrap()
});

static MADARA_LOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)locked chapter|cost to unlock|unlock this exciting chapter|get coins").unwrap()
});

static FEAT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)featured|chapter-featured").unwrap()
});

static SANITIZE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"[\\/*?:"<>|\r\n\t]+"#).unwrap()
});

static DUP_CH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)/chapter[-_]?(\d+(?:\.\d+)?)/chapter[-_]?(\d+(?:\.\d+)?)/?$").unwrap()
});

fn normalize_chapter_url(url: &str) -> String {
    if let Some(caps) = DUP_CH_RE.captures(url) {
        if caps.get(1).map(|m| m.as_str()) == caps.get(2).map(|m| m.as_str()) {
            return DUP_CH_RE.replace(url, "/chapter-$1").to_string();
        }
    }
    url.to_string()
}


#[derive(Parser, Debug, Clone)]
#[command(about = "AmoDL Telegram Downloader")]
struct Args {
    #[arg(long)]
    chat_id: String,

    #[arg(long)]
    chapter_url: String,

    #[arg(long, default_value = "Manga Chapter")]
    title: String,

    #[arg(long, default_value = "Chapter")]
    label: String,

    #[arg(long, default_value = "0")]
    job_id: String,

    #[arg(long, default_value = "")]
    status_msg_id: String,

    #[arg(long, default_value = "zip", value_parser = ["cbz", "zip"])]
    format: String,

    #[arg(long, default_value = "auto", value_parser = ["auto", "link_only"])]
    delivery_mode: String,

    #[arg(long, env = "TELEGRAM_BOT_TOKEN", default_value = "")]
    bot_token: String,

    #[arg(long, default_value = "")]
    callback_url: String,
}

#[derive(Debug, Clone, PartialEq)]
enum PageItem {
    Single(String),
    Strips(Vec<String>),
}

#[derive(Debug)]
#[allow(dead_code)]
enum AppError {
    ChapterLocked(String),
    CloudflareBlocked(String),
    Other(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::ChapterLocked(m) | AppError::CloudflareBlocked(m) | AppError::Other(m) => {
                write!(f, "{m}")
            }
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (count, c) in s.chars().rev().enumerate() {
        if count > 0 && count % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

#[allow(clippy::too_many_arguments)]
fn build_final_log(
    job_id: i64,
    chat_id: &str,
    title: &str,
    label: &str,
    chapter_url: &str,
    archive_format: &str,
    delivery_mode: &str,
    status: &str,
    duration_sec: f64,
    pages_found: usize,
    images_downloaded: usize,
    file_size: Option<u64>,
    error_msg: Option<&str>,
    traceback_str: Option<&str>,
) -> String {
    let size_str = file_size
        .map(|s| format!("{} bytes", format_with_commas(s)))
        .unwrap_or_else(|| "N/A".to_string());
    let now_str = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let summary = error_msg.unwrap_or("Download completed and delivered successfully.");

    let mut lines = vec![
        "=== AmoDL Final Download Report ===".to_string(),
        format!("Job ID: {job_id}"),
        format!("Chat ID: {chat_id}"),
        format!("Title: {title}"),
        format!("Chapter: {label}"),
        format!("URL: {chapter_url}"),
        format!("Format: {} | Delivery: {delivery_mode}", archive_format.to_uppercase()),
        format!("Status: {}", status.to_uppercase()),
        format!("Duration: {duration_sec:.1}s"),
        format!("Pages Discovered: {pages_found}"),
        format!("Pages Downloaded: {images_downloaded}"),
        format!("File Size: {size_str}"),
        format!("Timestamp: {now_str}"),
        String::new(),
        "--- [Result Summary] ---".to_string(),
        summary.to_string(),
    ];

    if let Some(tb) = traceback_str {
        if !tb.trim().is_empty() {
            lines.push(String::new());
            lines.push("--- [Traceback / Diagnostics] ---".to_string());
            lines.push(tb.trim().to_string());
        }
    }
    lines.join("\n")
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Manifest Loader & Page Extractor
// ─────────────────────────────────────────────────────────────────────────────

fn load_manifests(repo_root: &Path) -> Vec<Value> {
    let pattern = repo_root
        .join("resources")
        .join("extensions")
        .join("*.pcext.json");
    let mut manifests = Vec::new();
    if let Ok(paths) = glob::glob(&pattern.to_string_lossy()) {
        for entry in paths.flatten() {
            match fs::read_to_string(&entry)
                .and_then(|s| serde_json::from_str::<Value>(&s).map_err(Into::into))
            {
                Ok(data) => manifests.push(data),
                Err(e) => eprintln!("Error loading {}: {e}", entry.display()),
            }
        }
    }
    manifests
}

fn clean_host(url_str: &str) -> Option<String> {
    let parsed = Url::parse(url_str).ok()?;
    let host = parsed.host_str()?.to_lowercase();
    Some(host.strip_prefix("www.").unwrap_or(&host).to_string())
}

fn host_matches(host: &str, candidate: &str) -> bool {
    !candidate.is_empty() && (host == candidate || host.ends_with(&format!(".{candidate}")))
}

fn find_matching_manifest<'a>(manifests: &'a [Value], chapter_url: &str) -> Option<&'a Value> {
    let host = clean_host(chapter_url)?;
    for m in manifests {
        if let Some(mid) = m.get("id").and_then(|v| v.as_str()) {
            if host_matches(&host, &mid.to_lowercase()) {
                return Some(m);
            }
        }
        if let Some(base_url) = m.get("baseUrl").and_then(|v| v.as_str()) {
            if let Some(base_host) = clean_host(base_url) {
                if host_matches(&host, &base_host) {
                    return Some(m);
                }
            }
        }
        if let Some(alt_urls) = m
            .pointer("/config/alt_urls")
            .and_then(|v| v.as_array())
        {
            for alt in alt_urls.iter().filter_map(|v| v.as_str()) {
                if let Some(alt_host) = clean_host(alt) {
                    if host_matches(&host, &alt_host) {
                        return Some(m);
                    }
                }
            }
        }
    }
    None
}

fn dedup_strings(items: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|x| seen.insert(x.clone()))
        .collect()
}

async fn extract_mangadex_pages(client: &Client, chapter_url: &str) -> Vec<PageItem> {
    let cid = MANGADEX_RE1
        .captures(chapter_url)
        .or_else(|| MANGADEX_RE2.captures(chapter_url))
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
    let Some(cid) = cid else { return vec![] };

    let api_url = format!("https://api.mangadex.org/at-home/server/{cid}");
    let Ok(res) = client
        .get(&api_url)
        .header("User-Agent", "AmoDL/0.3")
        .timeout(Duration::from_secs(15))
        .send()
        .await
    else {
        return vec![];
    };
    if !res.status().is_success() {
        return vec![];
    }
    let Ok(data) = res.json::<Value>().await else {
        return vec![];
    };
    let base_url = data.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
    let ch_hash = data
        .pointer("/chapter/hash")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let Some(filenames) = data.pointer("/chapter/data").and_then(|v| v.as_array()) else {
        return vec![];
    };

    filenames
        .iter()
        .filter_map(|fn_val| fn_val.as_str())
        .map(|fn_str| PageItem::Single(format!("{base_url}/data/{ch_hash}/{fn_str}")))
        .collect()
}

async fn extract_senkuro_pages(client: &Client, chapter_url: &str) -> Vec<PageItem> {
    let mut mid: Option<String> = None;
    let mut cid: Option<String> = None;

    if let Some((m, c)) = chapter_url.split_once('|') {
        mid = Some(m.to_string());
        cid = Some(c.to_string());
    } else if let Ok(parsed) = Url::parse(chapter_url) {
        let path_parts: Vec<&str> = parsed.path().split('/').filter(|p| !p.is_empty()).collect();
        if let Some(ch_idx) = path_parts.iter().position(|&p| p == "chapter") {
            if ch_idx + 1 < path_parts.len() {
                cid = Some(path_parts[ch_idx + 1].to_string());
            }
            if ch_idx > 0 {
                let m_candidate = path_parts[ch_idx - 1];
                if m_candidate.starts_with("TUFOR0E") {
                    mid = Some(m_candidate.to_string());
                } else {
                    let q_search = "query($q:String,$o:Int){ mangaTachiyomiSearch(query:$q, offset:$o){ mangas { id slug } } }";
                    if let Ok(s_res) = client
                        .post("https://api.senkuro.com/graphql")
                        .header("User-Agent", USER_AGENT)
                        .json(&json!({
                            "query": q_search,
                            "variables": { "q": m_candidate.replace('-', " "), "o": 0 }
                        }))
                        .timeout(Duration::from_secs(15))
                        .send()
                        .await
                    {
                        if let Ok(s_data) = s_res.json::<Value>().await {
                            if let Some(mangas) = s_data
                                .pointer("/data/mangaTachiyomiSearch/mangas")
                                .and_then(|v| v.as_array())
                            {
                                for m in mangas {
                                    if m.get("slug").and_then(|v| v.as_str()) == Some(m_candidate) {
                                        mid = m.get("id").and_then(|v| v.as_str()).map(String::from);
                                        break;
                                    }
                                }
                                if mid.is_none() {
                                    mid = mangas
                                        .first()
                                        .and_then(|m| m.get("id"))
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                }
                            }
                        }
                    }
                }
            }
        } else if let Some(last) = path_parts.last() {
            cid = Some((*last).to_string());
        }
    }

    let (Some(mid), Some(cid)) = (mid, cid) else {
        return vec![];
    };

    let q_pages = "query($m:ID!,$c:ID!){ mangaTachiyomiChapterPages(mangaId:$m, chapterId:$c){ pages { url } } }";
    let Ok(p_res) = client
        .post("https://api.senkuro.com/graphql")
        .header("User-Agent", USER_AGENT)
        .json(&json!({
            "query": q_pages,
            "variables": { "m": mid, "c": cid }
        }))
        .timeout(Duration::from_secs(20))
        .send()
        .await
    else {
        return vec![];
    };
    let Ok(p_data) = p_res.json::<Value>().await else {
        return vec![];
    };
    let Some(pages) = p_data
        .pointer("/data/mangaTachiyomiChapterPages/pages")
        .and_then(|v| v.as_array())
    else {
        return vec![];
    };

    pages
        .iter()
        .filter_map(|p| p.get("url").and_then(|u| u.as_str()))
        .filter(|u| !u.is_empty() && !u.contains("tachiyomi-op"))
        .map(|u| PageItem::Single(u.to_string()))
        .collect()
}

async fn extract_webtoons_pages(client: &Client, chapter_url: &str) -> Vec<PageItem> {
    let Ok(res) = client
        .get(chapter_url)
        .header("User-Agent", USER_AGENT)
        .header("Referer", "https://www.webtoons.com/")
        .timeout(Duration::from_secs(20))
        .send()
        .await
    else {
        return vec![];
    };
    if !res.status().is_success() {
        return vec![];
    }
    let Ok(text) = res.text().await else {
        return vec![];
    };
    let doc = Html::parse_document(&text);
    let Ok(sel) = Selector::parse("#_imageList img, img._images, img.viewer_img") else {
        return vec![];
    };

    let mut urls = Vec::new();
    let mut seen = HashSet::new();
    for img in doc.select(&sel) {
        let el = img.value();
        let u = el
            .attr("data-url")
            .or_else(|| el.attr("data-src"))
            .or_else(|| el.attr("src"));
        if let Some(u) = u {
            if !u.starts_with("data:")
                && u.starts_with("http")
                && !AD_IMG_RE.is_match(u)
                && seen.insert(u.to_string())
            {
                urls.push(PageItem::Single(u.to_string()));
            }
        }
    }
    urls
}

async fn extract_naver_pages(client: &Client, chapter_url: &str) -> Result<Vec<PageItem>, AppError> {
    let mut target_url = chapter_url.to_string();
    if target_url.contains("m.comic.naver.com") {
        target_url = target_url.replace("m.comic.naver.com", "comic.naver.com");
    }

    // If it's a series list URL without &no=, fetch latest chapter via API
    if !target_url.contains("no=") {
        if let Ok(parsed) = Url::parse(&target_url) {
            let title_id = parsed
                .query_pairs()
                .find(|(k, _)| k == "titleId")
                .map(|(_, v)| v.to_string());
            if let Some(tid) = title_id {
                let api_url = format!("https://comic.naver.com/api/article/list?titleId={tid}&page=1&sort=DESC");
                if let Ok(res) = client
                    .get(&api_url)
                    .header("User-Agent", USER_AGENT)
                    .header("Referer", "https://comic.naver.com")
                    .send()
                    .await
                {
                    if let Ok(data) = res.json::<serde_json::Value>().await {
                        if let Some(articles) = data.pointer("/articleList").and_then(|v| v.as_array()) {
                            let target = articles.iter().find(|a| {
                                let charge = a.get("charge").and_then(|v| v.as_bool()).unwrap_or(false);
                                let lock = a.get("thumbnailLock").and_then(|v| v.as_bool()).unwrap_or(false);
                                !charge && !lock
                            }).or_else(|| articles.first());
                            if let Some(art) = target {
                                if let Some(no) = art.get("no").and_then(|v| v.as_i64()) {
                                    target_url = format!("https://comic.naver.com/webtoon/detail?titleId={tid}&no={no}");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let Ok(res) = client
        .get(&target_url)
        .header("User-Agent", USER_AGENT)
        .header("Referer", "https://comic.naver.com")
        .timeout(Duration::from_secs(20))
        .send()
        .await
    else {
        return Ok(vec![]);
    };

    if !res.status().is_success() {
        return Ok(vec![]);
    }

    // If detail URL was redirected back to /list, it means the chapter is locked/preview
    if target_url.contains("/detail") && res.url().path().contains("/list") {
        return Err(AppError::ChapterLocked(
            "این چپتر در ناور قفل است (نیاز به خرید با کوکی دارد یا جزو پیش‌نمایش‌های پولی است)."
                .to_string(),
        ));
    }

    let Ok(text) = res.text().await else {
        return Ok(vec![]);
    };

    if (text.contains("유료회차") || text.contains("쿠키")) && !text.contains("wt_viewer") {
        return Err(AppError::ChapterLocked("این چپتر در ناور قفل (پولی / نیاز به کوکی) است.".to_string()));
    }

    let doc = Html::parse_document(&text);
    let mut urls = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(sel) = Selector::parse(".wt_viewer img, #comic_view_area img") {
        for el in doc.select(&sel) {
            let v = el.value();
            let u = v.attr("src").or_else(|| v.attr("data-src"));
            if let Some(u) = u {
                if u.starts_with("http")
                    && u.contains("image-comic.pstatic.net")
                    && !u.contains("thumbnail")
                    && !u.contains("banner")
                    && !u.contains("/static/")
                    && seen.insert(u.to_string())
                {
                    urls.push(PageItem::Single(u.trim().to_string()));
                }
            }
        }
    }

    if urls.is_empty() {
        static NAVER_FALLBACK_RE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?i)https?://image-comic\.pstatic\.net/webtoon/[^"'\s<>]+\.(?:jpg|jpeg|png|webp)"#).unwrap()
        });
        for m in NAVER_FALLBACK_RE.find_iter(&text) {
            let u = m.as_str();
            if !u.contains("thumbnail") && !u.contains("banner") && !u.contains("/static/") && seen.insert(u.to_string()) {
                urls.push(PageItem::Single(u.to_string()));
            }
        }
    }

    Ok(urls)
}

async fn extract_hipmh_pages(chapter_url: &str, scripts_dir: &Path) -> Vec<PageItem> {
    let decoder_file = scripts_dir.join("hipmh_decoder.js");
    let js_code = r#"
    const fs = require('fs');
    function deriveApiHid(url) {
      let slug = null;
      try {
        const u = new URL(url);
        if (u.searchParams.get('hid')) {
          slug = u.searchParams.get('hid');
        } else if (url.includes('/chapter/')) {
          slug = url.split('/chapter/')[1]?.split(/[?#]/)[0];
        }
      } catch (e) {
        if (url.includes('/chapter/')) {
          slug = url.split('/chapter/')[1]?.split(/[?#]/)[0];
        }
      }
      if (!slug) return null;
      slug = slug.replace(/\/+$/, '');
      if (!slug.includes('-')) return slug;
      const parts = slug.split('-');
      try {
        const p1 = Buffer.from(parts[0], 'base64').toString();
        const cMatch = p1.match(/c:(\d+)/);
        if (!cMatch) return slug;
        const cB64 = Buffer.from('c:' + cMatch[1]).toString('base64').replace(/=+$/, '');
        return cB64 + '-' + parts.slice(1).join('-');
      } catch (e) {
        return slug;
      }
    }

    const D = (40503 << 16 | 31153) >>> 0, R = (34283 << 16 | 51819) >>> 0;
    function q(t) {
      if (t == null) return null;
      const l = Number(t);
      return !Number.isFinite(l) || l < 0 || l > 4294967295 ? null : Math.floor(l) >>> 0;
    }
    function removeHoneypot(t, l, u) {
      if (!Array.isArray(t) || t.length <= 0) return t;
      const d = t.length, m = q(l), g = q(u);
      if (m === null || g === null) return t;
      try {
        const L = BigInt(g) * BigInt(D), v = BigInt(d) * BigInt(R);
        let b = Number((L ^ v) % BigInt(d));
        b < 0 && (b += d);
        const h = Number(BigInt(m) ^ BigInt(b));
        if (h < 0 || h >= d) return t;
        const c = [...t];
        c.splice(h, 1);
        return c;
      } catch (e) {
        return t;
      }
    }

    async function run() {
      const chapterUrl = process.env.CHAPTER_URL;
      const userAgent = process.env.USER_AGENT;
      const decoderPath = process.env.DECODER_PATH;
      const imgBase = 'https://hip-cf-1.s3imgs.top';
      const apiHid = deriveApiHid(chapterUrl);
      if (!apiHid) {
        console.error('Failed to derive apiHid from URL:', chapterUrl);
        return [];
      }
      let decCode = '';
      if (fs.existsSync(decoderPath)) {
        decCode = fs.readFileSync(decoderPath, 'utf8');
      } else {
        decCode = await fetch('https://reader.hipmh.top/assets/runtime/chapter-decoder.js').then(r => r.text());
      }
      const vm = require('vm');
      const win = {
        atob: (s) => Buffer.from(s, 'base64').toString('binary'),
        btoa: (s) => Buffer.from(s, 'binary').toString('base64'),
        TextDecoder,
        TextEncoder,
      };
      win.window = win;
      win.self = win;
      win.globalThis = win;
      const sandbox = {
        ...win,
        window: win,
        self: win,
        globalThis: win,
        atob: win.atob,
        btoa: win.btoa,
        TextDecoder,
        TextEncoder,
        Uint8Array,
        ArrayBuffer,
        Buffer,
        console,
      };
      vm.createContext(sandbox);
      vm.runInContext(decCode, sandbox, { timeout: 5000 });
      const decodeFn = sandbox.window.__cimg ? sandbox.window.__cimg.r : null;
      if (!decodeFn) {
        console.error('HipMH decoder function not found');
        return [];
      }
      const apiUrl = 'https://hipapi1.s3file.top/v2/chapter?hid=' + encodeURIComponent(apiHid);
      const data = await fetch(apiUrl, { headers: { 'User-Agent': userAgent, 'Referer': chapterUrl } }).then(r => r.json());
      if (!data || !data.data || !data.data.images) {
        console.error('HipMH API did not return images:', JSON.stringify(data));
        return [];
      }
      const decoded = await decodeFn(data.data.images);
      const cleaned = removeHoneypot(decoded, data.data.order_id, data.data.sid);
      return cleaned.map(p => p.startsWith('http') ? p : imgBase + p);
    }
    run().then(res => console.log(JSON.stringify(res))).catch(err => {
      console.error('HipMH decode exception:', err);
      console.log('[]');
    });
    "#;

    let child = Command::new("node")
        .arg("-e")
        .arg(js_code)
        .env("CHAPTER_URL", chapter_url)
        .env("USER_AGENT", USER_AGENT)
        .env("DECODER_PATH", decoder_file.to_string_lossy().as_ref())
        .kill_on_drop(true)
        .output();

    match tokio::time::timeout(Duration::from_secs(35), child).await {
        Ok(Ok(output)) => {
            if !output.stderr.is_empty() {
                eprintln!("HipMH stderr: {}", String::from_utf8_lossy(&output.stderr));
            }
            if let Ok(urls) = serde_json::from_slice::<Vec<String>>(&output.stdout) {
                return urls.into_iter().map(PageItem::Single).collect();
            }
        }
        Ok(Err(e)) => eprintln!("HipMH decode error: {e}"),
        Err(_) => eprintln!("HipMH node process timed out"),
    }
    vec![]
}

fn is_cloudflare_challenge(html_text: &str) -> bool {
    if html_text.is_empty() {
        return false;
    }
    const CF_SIGNALS: &[&str] = &[
        "<title>Attention Required! | Cloudflare</title>",
        "<title>Just a moment...</title>",
        "cf-turnstile",
        "cf-challenge",
        "_cf_translation",
        "cf-browser-verification",
        "challenges.cloudflare.com",
    ];
    CF_SIGNALS.iter().any(|sig| html_text.contains(sig))
}

fn resolve_url(base: &str, rel: &str) -> String {
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return rel.to_string();
    }
    if let Ok(base_url) = Url::parse(base) {
        if let Ok(joined) = base_url.join(rel) {
            return joined.to_string();
        }
    }
    rel.to_string()
}

fn extract_ts_reader_pages(html: &str, chapter_url: &str) -> Result<Vec<String>, AppError> {
    let Some(start) = html.find("ts_reader.run(") else {
        return Ok(vec![]);
    };
    let after_open = start + "ts_reader.run(".len();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    let mut end_idx = None;

    for (offset, c) in html[after_open..].char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end_idx = Some(after_open + offset + c.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }

    let Some(end) = end_idx else {
        return Ok(vec![]);
    };

    let Ok(data) = serde_json::from_str::<Value>(&html[after_open..end]) else {
        return Ok(vec![]);
    };
    if data.get("protected").and_then(|v| v.as_bool()) == Some(true) {
        return Err(AppError::ChapterLocked(
            "این چپتر توسط سایت قفل و محافظت شده است.".to_string(),
        ));
    }

    if let Some(sources) = data.get("sources").and_then(|v| v.as_array()) {
        for src in sources {
            if let Some(imgs) = src.get("images").and_then(|v| v.as_array()) {
                let mut valid = Vec::new();
                for u in imgs.iter().filter_map(|v| v.as_str()) {
                    let u_clean = u.trim();
                    if u_clean.is_empty()
                        || u_clean.starts_with("data:")
                        || AD_IMG_RE.is_match(u_clean)
                    {
                        continue;
                    }
                    let final_u = if !chapter_url.is_empty() {
                        resolve_url(chapter_url, u_clean)
                    } else {
                        u_clean.to_string()
                    };
                    valid.push(final_u);
                }
                if !valid.is_empty() {
                    return Ok(dedup_strings(valid));
                }
            }
        }
    }
    Ok(vec![])
}

async fn extract_pages_api(
    client: &Client,
    chapter_url: &str,
    manifest: &Value,
) -> Vec<PageItem> {
    let Some(pages_api) = manifest.pointer("/config/pagesApi") else {
        return vec![];
    };
    if pages_api
        .get("jsDecoder")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return vec![];
    }

    let url_parts: Vec<&str> = chapter_url.trim_end_matches('/').split('/').collect();
    let chapter_slug = url_parts.last().copied().unwrap_or("");
    let manga_slug = if url_parts.len() >= 2 {
        url_parts[url_parts.len() - 2]
    } else {
        ""
    };
    let base_url = manifest
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_end_matches('/');

    let Some(url_tpl) = pages_api.get("url").and_then(|v| v.as_str()) else {
        return vec![];
    };

    let api_url = url_tpl
        .replace("{chapterSlug}", chapter_slug)
        .replace("{mangaSlug}", manga_slug)
        .replace("{baseUrl}", base_url);

    let ua = manifest
        .get("user_agent")
        .and_then(|v| v.as_str())
        .unwrap_or(USER_AGENT);

    let req = if let Some(body_obj) = pages_api.get("body").and_then(|v| v.as_object()) {
        let mut form_data = HashMap::new();
        for (k, v) in body_obj {
            if let Some(v_str) = v.as_str() {
                form_data.insert(
                    k.clone(),
                    v_str
                        .replace("{chapterSlug}", chapter_slug)
                        .replace("{mangaSlug}", manga_slug)
                        .replace("{baseUrl}", base_url),
                );
            }
        }
        client.post(&api_url).form(&form_data)
    } else {
        client.get(&api_url)
    };

    let Ok(res) = req
        .header("User-Agent", ua)
        .header("Referer", chapter_url)
        .header("X-Requested-With", "XMLHttpRequest")
        .timeout(Duration::from_secs(20))
        .send()
        .await
    else {
        return vec![];
    };

    if !res.status().is_success() {
        return vec![];
    }

    let Ok(data) = res.json::<Value>().await else {
        return vec![];
    };

    let items_path = pages_api
        .get("items")
        .and_then(|v| v.as_str())
        .unwrap_or("data");
    let mut cur = &data;
    for p in items_path.split('.') {
        if let Some(next) = cur.get(p) {
            cur = next;
        } else {
            return vec![];
        }
    }

    let mut urls = Vec::new();
    if let Some(arr) = cur.as_array() {
        let img_field = pages_api.get("image_field").and_then(|v| v.as_str());
        for it in arr {
            if let Some(s) = it.as_str() {
                let u = s.trim();
                if !u.is_empty() {
                    urls.push(resolve_url(base_url, u));
                }
            } else if let (Some(field), Some(obj)) = (img_field, it.as_object()) {
                if let Some(s) = obj.get(field).and_then(|v| v.as_str()) {
                    let u = s.trim();
                    if !u.is_empty() {
                        urls.push(resolve_url(base_url, u));
                    }
                }
            }
        }
    }

    dedup_strings(urls).into_iter().map(PageItem::Single).collect()
}

fn extract_valir_or_rsc_pages(html_text: &str, base_url: &str) -> Vec<String> {
    let mut pages = Vec::new();
    for cap in VALIR_RE.captures_iter(html_text) {
        if let Some(m) = cap.get(1) {
            let mut clean_url = m
                .as_str()
                .replace(r"\/", "/")
                .replace(r#"\""#, "\"")
                .trim()
                .to_string();
            if clean_url.starts_with("//") {
                clean_url = format!("https:{clean_url}");
            } else if !clean_url.starts_with("http") {
                clean_url = resolve_url(base_url, &clean_url);
            }
            if !AD_IMG_RE.is_match(&clean_url) {
                pages.push(clean_url);
            }
        }
    }
    let deduped = dedup_strings(pages);
    if deduped.len() >= 2 {
        return deduped;
    }

    // Fallback: Next.js RSC manga series images (e.g. Kayn Scans, ElfToon)
    let mut series_pages = Vec::new();
    for cap in NEXT_SERIES_IMG_RE.captures_iter(html_text) {
        if let Some(m) = cap.get(0) {
            let path = m.as_str();
            let lower = path.to_ascii_lowercase();
            if lower.contains("/cover-")
                || lower.contains("avatar")
                || lower.contains("logo")
                || lower.contains("thumb")
                || lower.contains("ogimage")
            {
                continue;
            }
            series_pages.push(path.to_string());
        }
    }

    let p_pages: Vec<String> = series_pages
        .iter()
        .filter(|s| s.contains("/p-"))
        .cloned()
        .collect();
    let chosen = if p_pages.len() >= 2 {
        p_pages
    } else {
        series_pages
    };

    let resolved: Vec<String> = chosen
        .into_iter()
        .map(|rel| resolve_url(base_url, &rel))
        .collect();
    let deduped = dedup_strings(resolved);
    if deduped.len() >= 2 {
        deduped
    } else {
        vec![]
    }
}

async fn extract_ajax_pages(
    client: &Client,
    html_text: &str,
    chapter_url: &str,
    manifest: Option<&Value>,
) -> Vec<PageItem> {
    let Some(cfg) = manifest.and_then(|m| m.pointer("/config/ajax_pages")) else {
        return vec![];
    };
    let id_rx = cfg.get("id_rx").and_then(|v| v.as_str());
    let Some(url_template) = cfg.get("url").and_then(|v| v.as_str()) else {
        return vec![];
    };
    let items_key = cfg.get("items").and_then(|v| v.as_str()).unwrap_or("pages");
    let field_key = cfg
        .get("image_field")
        .and_then(|v| v.as_str())
        .unwrap_or("imageUrl");

    let mut ch_id: Option<String> = None;
    if let Some(rx_str) = id_rx {
        if let Ok(re) = Regex::new(rx_str) {
            ch_id = re
                .captures(html_text)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
        }
    }
    if ch_id.is_none() {
        ch_id = AJAX_RE_CH
            .captures(html_text)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
    }
    if ch_id.is_none() {
        ch_id = AJAX_RE_FALLBACK
            .captures(html_text)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
    }

    let Some(ch_id) = ch_id else { return vec![] };
    let Ok(parsed) = Url::parse(chapter_url) else {
        return vec![];
    };
    let base = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""));
    let api_url = url_template
        .replace("{baseUrl}", &base)
        .replace("{id}", &ch_id);

    let Ok(res) = client
        .get(&api_url)
        .header("User-Agent", USER_AGENT)
        .header("Referer", chapter_url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
    else {
        return vec![];
    };
    if !res.status().is_success() {
        return vec![];
    }
    let Ok(data) = res.json::<Value>().await else {
        return vec![];
    };

    let items = if let Some(obj) = data.as_object() {
        obj.get(items_key).and_then(|v| v.as_array()).cloned().unwrap_or_default()
    } else if let Some(arr) = data.as_array() {
        arr.clone()
    } else {
        vec![]
    };

    let strip_cfg = cfg.get("strip_join");
    let mut pages = Vec::new();

    for it in &items {
        if let (Some(scfg), Some(it_obj)) = (strip_cfg, it.as_object()) {
            let parts_key = scfg
                .get("parts")
                .and_then(|v| v.as_str())
                .unwrap_or("strips");
            if let Some(strips) = it_obj.get(parts_key).and_then(|v| v.as_array()) {
                if strips.len() > 1 {
                    let strip_field = scfg
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("imageUrl");
                    let order_field = scfg
                        .get("order")
                        .and_then(|v| v.as_str())
                        .unwrap_or("xOffsetPct");
                    let mut sorted_strips = strips.clone();
                    sorted_strips.sort_by(|a, b| {
                        let oa = a.get(order_field).and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let ob = b.get(order_field).and_then(|v| v.as_f64()).unwrap_or(0.0);
                        oa.partial_cmp(&ob).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    let strip_urls: Vec<String> = sorted_strips
                        .iter()
                        .filter_map(|s| s.get(strip_field).and_then(|v| v.as_str()))
                        .map(|u| resolve_url(&base, u.trim()))
                        .collect();
                    if strip_urls.len() >= 2 {
                        pages.push(PageItem::Strips(strip_urls));
                        continue;
                    }
                }
            }
        }

        let raw_u = if let Some(obj) = it.as_object() {
            obj.get(field_key).and_then(|v| v.as_str())
        } else {
            it.as_str()
        };

        if let Some(raw_u) = raw_u {
            let u = resolve_url(&base, raw_u.trim());
            if u.starts_with("http") && !AD_IMG_RE.is_match(&u) {
                pages.push(PageItem::Single(u));
            }
        }
    }

    if pages.len() >= 2 {
        pages
    } else {
        vec![]
    }
}

async fn extract_html_pages(
    client: &Client,
    mut chapter_url: String,
    manifest: Option<&Value>,
) -> Result<Vec<PageItem>, AppError> {
    if let Some(m) = manifest {
        if m.pointer("/config/pagesApi").is_some() {
            let api_pages = extract_pages_api(client, &chapter_url, m).await;
            if !api_pages.is_empty() {
                return Ok(api_pages);
            }
        }
    }

    let mut html_text = String::new();
    if let Ok(res) = client
        .get(&chapter_url)
        .header("User-Agent", USER_AGENT)
        .header("Referer", &chapter_url)
        .timeout(Duration::from_secs(25))
        .send()
        .await
    {
        if res.status().is_success() {
            chapter_url = res.url().to_string();
            html_text = res.text().await.unwrap_or_default();
        }
    }

    // shortcut: uses system curl binary as fallback instead of Python curl_cffi Chrome JA3 impersonation, upgrade to `rquest` crate if strict JA3 spoofing is needed.
    if html_text.is_empty() {
        if let Ok(out) = Command::new("curl")
            .args(["-sSL", "--max-time", "25", "-A", USER_AGENT, &chapter_url])
            .output()
            .await
        {
            let body = String::from_utf8_lossy(&out.stdout).to_string();
            if is_cloudflare_challenge(&body) {
                return Err(AppError::CloudflareBlocked(
                    "این سورس دارای سیستم ضدربات Cloudflare Turnstile است و در حال حاضر دانلود خودکار آن مسدود می‌باشد.".to_string(),
                ));
            }
            if !body.is_empty() {
                html_text = body;
            }
        }
    }

    if html_text.is_empty() {
        return Ok(vec![]);
    }

    if is_cloudflare_challenge(&html_text) {
        return Err(AppError::CloudflareBlocked(
            "این سورس دارای سیستم ضدربات Cloudflare Turnstile است و در حال حاضر دانلود خودکار آن مسدود می‌باشد.".to_string(),
        ));
    }


    let ts_pages = extract_ts_reader_pages(&html_text, &chapter_url)?;
    if !ts_pages.is_empty() {
        return Ok(ts_pages.into_iter().map(PageItem::Single).collect());
    }

    let rsc_pages = extract_valir_or_rsc_pages(&html_text, &chapter_url);
    if rsc_pages.len() >= 2 {
        return Ok(rsc_pages.into_iter().map(PageItem::Single).collect());
    }

    let ajax_pages = extract_ajax_pages(client, &html_text, &chapter_url, manifest).await;
    if ajax_pages.len() >= 2 {
        return Ok(ajax_pages);
    }

    let doc = Html::parse_document(&html_text);

    // GenzToons / Meowing uid reader pattern
    if let Ok(uid_sel) = Selector::parse("#pages img[uid], img.myImage[uid], div#pages img[uid]") {
        let mut uid_urls = Vec::new();
        for el in doc.select(&uid_sel) {
            let v = el.value();
            if let Some(uid) = v.attr("uid").or_else(|| v.attr("data-uid")) {
                let clean_uid = uid.trim();
                if !clean_uid.is_empty() {
                    let full_url = if clean_uid.starts_with("http") {
                        clean_uid.to_string()
                    } else {
                        format!("https://cdn.meowing.org/uploads/{clean_uid}")
                    };
                    uid_urls.push(full_url);
                }
            }
        }
        let deduped = dedup_strings(uid_urls);
        if deduped.len() >= 2 {
            return Ok(deduped.into_iter().map(PageItem::Single).collect());
        }
    }
    if let Some(pages_cfg) = manifest.and_then(|m| m.get("pages")) {
        if let Some(sel_str) = pages_cfg.get("image_selector").and_then(|v| v.as_str()) {
            let attr = pages_cfg
                .get("image_attr")
                .and_then(|v| v.as_str())
                .unwrap_or("src");
            if let Ok(sel) = Selector::parse(sel_str) {
                let mut urls = Vec::new();
                for el in doc.select(&sel) {
                    let v = el.value();
                    let u = v
                        .attr(attr)
                        .or_else(|| v.attr("data-src"))
                        .or_else(|| v.attr("data-lazy-src"))
                        .or_else(|| v.attr("data-cfsrc"))
                        .or_else(|| v.attr("src"));
                    if let Some(u) = u {
                        if !u.starts_with("data:") && !AD_IMG_RE.is_match(u) {
                            urls.push(resolve_url(&chapter_url, u.trim()));
                        }
                    }
                }
                let deduped = dedup_strings(urls);
                if deduped.len() >= 2 {
                    return Ok(deduped.into_iter().map(PageItem::Single).collect());
                }
            }
        }
    }

    let candidates: &[(&str, &[&str])] = &[
        (
            ".comic-contain img, .comic-contain amp-img, amp-img.comic-contain__item",
            &["data-src", "src"],
        ),
        (
            "#readerarea img, .readerarea img, div#readerarea img",
            &["data-src", "data-lazy-src", "src", "data-original"],
        ),
        (
            "div.reading-content img, .wp-manga-chapter-img",
            &["data-src", "data-lazy-src", "src"],
        ),
        (
            "div#chapter-content img, div.chapter-images img",
            &["data-src", "data-lazy-src", "src"],
        ),
        (
            ".page-break img, .chapter-image img, div[class*='page'] img",
            &["data-src", "data-lazy-src", "src"],
        ),
        (
            "#chapter-reader img, img.chapter-img, div.reader-area img, img.mgr-page",
            &["data-src", "data-lazy-src", "src"],
        ),
        (
            "img[src*='chapter'], img[data-src*='chapter'], img.imgholder",
            &["data-src", "src"],
        ),
    ];

    for (sel_str, attrs) in candidates {
        if let Ok(sel) = Selector::parse(sel_str) {
            let mut urls = Vec::new();
            for el in doc.select(&sel) {
                let v = el.value();
                let u = attrs.iter().find_map(|a| v.attr(a));
                if let Some(u) = u {
                    if !u.starts_with("data:") && !AD_IMG_RE.is_match(u) {
                        urls.push(resolve_url(&chapter_url, u.trim()));
                    }
                }
            }
            let deduped = dedup_strings(urls);
            if deduped.len() >= 2 {
                return Ok(deduped.into_iter().map(PageItem::Single).collect());
            }
        }
    }

    // If no reader pages were found, verify if chapter is locked/premium
    let lock_selectors = [
        ".lock-container",
        ".lock-card",
        ".lock-status",
        ".fa-coins",
        "[class*='chapter-lock']",
        "[class*='coin-lock']",
        "[class*='early-access']",
        "[class*='badge-locked']",
        "[data-bs-target*='locked']",
    ];
    for ls in lock_selectors {
        if let Ok(sel) = Selector::parse(ls) {
            if let Some(el) = doc.select(&sel).next() {
                let txt: String = el.text().collect::<Vec<_>>().join(" ").trim().to_string();
                let detail = if txt.is_empty() {
                    String::new()
                } else {
                    let truncated: String = txt.chars().take(80).collect();
                    format!(" ({truncated})")
                };
                return Err(AppError::ChapterLocked(format!("این چپتر قفل است{detail}")));
            }
        }
    }

    if LOCK_RE.is_match(&html_text) {
        return Err(AppError::ChapterLocked(
            "این چپتر قفل/پولی (Premium یا نیازمند سکه) است و هنوز رایگان نشده است.".to_string(),
        ));
    }

    if let Ok(title_sel) = Selector::parse("title") {
        if let Some(title_el) = doc.select(&title_sel).next() {
            let title_txt: String = title_el.text().collect();
            if PREM_RE.is_match(&title_txt) {
                return Err(AppError::ChapterLocked(
                    "این چپتر پولی (Premium/Early Access) است و هنوز رایگان نشده است."
                        .to_string(),
                ));
            }
        }
    }

    Ok(vec![])
}

async fn extract_vortex_pages(client: &Client, chapter_url: &str) -> Result<Vec<PageItem>, AppError> {
    let Ok(res) = client
        .get(chapter_url)
        .header("User-Agent", USER_AGENT)
        .header("Referer", "https://vortexscans.org/")
        .timeout(Duration::from_secs(25))
        .send()
        .await
    else {
        return Ok(vec![]);
    };
    if !res.status().is_success() {
        return Ok(vec![]);
    }
    let Ok(text) = res.text().await else {
        return Ok(vec![]);
    };

    if MADARA_LOCK_RE.is_match(&text) {
        return Err(AppError::ChapterLocked(
            "این چپتر قفل است (نیاز به سکه برای آزادسازی دارد).".to_string(),
        ));
    }

    let doc = Html::parse_document(&text);
    let mut urls = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(sel) = Selector::parse("img[data-reader-page-image='true'], img[alt*='Page']") {
        for img in doc.select(&sel) {
            let v = img.value();
            let u = v
                .attr("src")
                .or_else(|| v.attr("data-src"))
                .or_else(|| v.attr("data-cfsrc"))
                .or_else(|| v.attr("data-lazy-src"));
            if let Some(u) = u {
                let u_trim = u.trim();
                if !u_trim.starts_with("data:")
                    && u_trim.starts_with("http")
                    && u_trim.contains("upload/series/")
                    && !AD_IMG_RE.is_match(u_trim)
                    && !FEAT_RE.is_match(u_trim)
                    && seen.insert(u_trim.to_string())
                {
                    urls.push(PageItem::Single(u_trim.to_string()));
                }
            }
        }
    }

    if urls.is_empty() {
        if let Ok(sel) = Selector::parse("img") {
            for img in doc.select(&sel) {
                let v = img.value();
                let u = v.attr("src").or_else(|| v.attr("data-src")).unwrap_or("");
                let u_trim = u.trim();
                if u_trim.contains("storage.vortexscans.org/upload/series/")
                    && !AD_IMG_RE.is_match(u_trim)
                    && !FEAT_RE.is_match(u_trim)
                    && seen.insert(u_trim.to_string())
                {
                    urls.push(PageItem::Single(u_trim.to_string()));
                }
            }
        }
    }

    Ok(urls)
}

async fn extract_asura_pages(client: &Client, chapter_url: &str) -> Result<Vec<PageItem>, AppError> {
    let Ok(parsed) = Url::parse(chapter_url) else {
        return Ok(vec![]);
    };
    let segments: Vec<&str> = parsed.path().split('/').filter(|s| !s.is_empty()).collect();
    let mut slug = None;
    let mut ch_num = None;

    for (idx, &s) in segments.iter().enumerate() {
        let s_lower = s.to_lowercase();
        if (s_lower == "comics" || s_lower == "series") && idx + 1 < segments.len() {
            slug = Some(segments[idx + 1].to_lowercase());
        }
        if (s_lower == "chapter" || s_lower == "ch") && idx + 1 < segments.len() {
            let next = segments[idx + 1].to_lowercase();
            ch_num = Some(
                next.strip_prefix("chapter-")
                    .or_else(|| next.strip_prefix("ch-"))
                    .unwrap_or(&next)
                    .to_string(),
            );
        } else if s_lower.starts_with("chapter-") {
            ch_num = Some(s_lower.strip_prefix("chapter-").unwrap_or(&s_lower).to_string());
        } else if s_lower.starts_with("ch-") {
            ch_num = Some(s_lower.strip_prefix("ch-").unwrap_or(&s_lower).to_string());
        }
    }

    if slug.is_none() && segments.len() >= 2 {
        slug = Some(segments[segments.len() - 2].to_lowercase());
    }
    if ch_num.is_none() && !segments.is_empty() {
        let last = segments.last().copied().unwrap_or("").to_lowercase();
        ch_num = Some(
            last.strip_prefix("chapter-")
                .or_else(|| last.strip_prefix("ch-"))
                .unwrap_or(&last)
                .to_string(),
        );
    }

    let (Some(slug), Some(ch_num)) = (slug, ch_num) else {
        return Ok(vec![]);
    };

    let mut slugs_to_try = vec![slug.clone()];
    if let Some(pos) = slug.rfind('-') {
        let suffix = &slug[pos + 1..];
        if suffix.len() >= 6 && suffix.chars().all(|c| c.is_ascii_hexdigit()) {
            slugs_to_try.push(slug[..pos].to_string());
        }
    }

    for s_try in &slugs_to_try {
        let api_url = format!("https://api.asurascans.com/api/series/{s_try}/chapters/{ch_num}");
        let Ok(res) = client
            .get(&api_url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(15))
            .send()
            .await
        else {
            continue;
        };

        if res.status().is_success() {
            if let Ok(data) = res.json::<Value>().await {
                if let Some(chap) = data.pointer("/data/chapter") {
                    if chap.get("is_premium").and_then(|v| v.as_bool()) == Some(true) {
                        let until_str = chap
                            .get("early_access_until")
                            .and_then(|v| v.as_str())
                            .map(|u| format!(" (زمان آزادسازی: {u})"))
                            .unwrap_or_default();
                        return Err(AppError::ChapterLocked(format!(
                            "این چپتر در Asura Scans پولی (Early Access) است و هنوز رایگان نشده است{until_str}."
                        )));
                    }
                    if let Some(raw_pages) = chap.get("pages").and_then(|v| v.as_array()) {
                        let mut pages = Vec::new();
                        for p in raw_pages {
                            if let Some(u) = p.get("url").and_then(|v| v.as_str()) {
                                pages.push(PageItem::Single(u.trim().to_string()));
                            } else if let Some(u) = p.as_str() {
                                pages.push(PageItem::Single(u.trim().to_string()));
                            }
                        }
                        if !pages.is_empty() {
                            return Ok(pages);
                        }
                    }
                }
            }
        }
    }

    // Fallback: Query chapter list to find by number/slug
    for s_try in &slugs_to_try {
        let list_url = format!("https://api.asurascans.com/api/series/{s_try}/chapters");
        let Ok(res) = client
            .get(&list_url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(15))
            .send()
            .await
        else {
            continue;
        };

        if res.status().is_success() {
            if let Ok(data) = res.json::<Value>().await {
                if let Some(chapters) = data.get("data").and_then(|v| v.as_array()) {
                    let target_num = ch_num.parse::<f64>().ok();
                    for c in chapters {
                        let matches_num = target_num.is_some() && c.get("number").and_then(|v| v.as_f64()) == target_num;
                        let matches_slug = c.get("slug").and_then(|v| v.as_str()) == Some(&ch_num)
                            || c.get("slug").and_then(|v| v.as_str()) == Some(&format!("chapter-{ch_num}"));
                        if matches_num || matches_slug {
                            if c.get("is_premium").and_then(|v| v.as_bool()) == Some(true) {
                                let until_str = c
                                    .get("early_access_until")
                                    .and_then(|v| v.as_str())
                                    .map(|u| format!(" (زمان آزادسازی: {u})"))
                                    .unwrap_or_default();
                                return Err(AppError::ChapterLocked(format!(
                                    "این چپتر در Asura Scans پولی (Early Access) است و هنوز رایگان نشده است{until_str}."
                                )));
                            }
                            if let Some(chap_id) = c.get("id").and_then(|v| v.as_i64()) {
                                let c_url = format!("https://api.asurascans.com/api/series/{s_try}/chapters/{chap_id}");
                                if let Ok(c_res) = client
                                    .get(&c_url)
                                    .header("User-Agent", USER_AGENT)
                                    .header("Accept", "application/json")
                                    .timeout(Duration::from_secs(15))
                                    .send()
                                    .await
                                {
                                    if c_res.status().is_success() {
                                        if let Ok(c_data) = c_res.json::<Value>().await {
                                            if let Some(raw_pages) = c_data.pointer("/data/chapter/pages").and_then(|v| v.as_array()) {
                                                let mut pages = Vec::new();
                                                for p in raw_pages {
                                                    if let Some(u) = p.get("url").and_then(|v| v.as_str()) {
                                                        pages.push(PageItem::Single(u.trim().to_string()));
                                                    } else if let Some(u) = p.as_str() {
                                                        pages.push(PageItem::Single(u.trim().to_string()));
                                                    }
                                                }
                                                if !pages.is_empty() {
                                                    return Ok(pages);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(vec![])
}

async fn get_chapter_page_urls(
    client: &Client,
    chapter_url: &str,
    repo_root: &Path,
) -> Result<Vec<PageItem>, AppError> {
    let host = Url::parse(chapter_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .unwrap_or_default();

    if host.contains("asurascans.com") || host.contains("asuracomic.net") {
        let pages = extract_asura_pages(client, chapter_url).await?;
        if !pages.is_empty() {
            return Ok(pages);
        }
    }
    if host.contains("mangadex.org") {
        let pages = extract_mangadex_pages(client, chapter_url).await;
        if !pages.is_empty() {
            return Ok(pages);
        }
    }
    if host.contains("senkuro.com") {
        let pages = extract_senkuro_pages(client, chapter_url).await;
        if !pages.is_empty() {
            return Ok(pages);
        }
    }
    if host.contains("webtoons.com") {
        let pages = extract_webtoons_pages(client, chapter_url).await;
        if !pages.is_empty() {
            return Ok(pages);
        }
    }
    if host.contains("comic.naver.com") {
        let pages = extract_naver_pages(client, chapter_url).await?;
        if !pages.is_empty() {
            return Ok(pages);
        }
    }
    if host.contains("hipmh") {
        let scripts_dir = repo_root.join("scripts");
        let pages = extract_hipmh_pages(chapter_url, &scripts_dir).await;
        if !pages.is_empty() {
            return Ok(pages);
        }
    }
    if host.contains("vortexscans.org") {
        let pages = extract_vortex_pages(client, chapter_url).await?;
        if !pages.is_empty() {
            return Ok(pages);
        }
    }

    let mut target_url = chapter_url.to_string();
    if host.contains("mgeko") && target_url.contains("/manga/") {
        if let Some(pos) = target_url.find("/chapter-") {
            let (manga_base, ch_part) = target_url.split_at(pos);
            let ch_num_str = ch_part.trim_start_matches("/chapter-").trim_matches('/');
            let all_chapters_url = format!("{manga_base}/all-chapters/");
            if let Ok(res) = client
                .get(&all_chapters_url)
                .header("User-Agent", USER_AGENT)
                .timeout(Duration::from_secs(10))
                .send()
                .await
            {
                if let Ok(html) = res.text().await {
                    let pattern = format!("chapter-{ch_num_str}");
                    let doc = Html::parse_document(&html);
                    if let Ok(sel) = Selector::parse("a[href*='reader/en']") {
                        for el in doc.select(&sel) {
                            if let Some(href) = el.value().attr("href") {
                                if href.contains(&pattern) {
                                    target_url = resolve_url(&all_chapters_url, href);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let manifests = load_manifests(repo_root);
    let manifest = find_matching_manifest(&manifests, &target_url);
    extract_html_pages(client, target_url, manifest).await
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Live Telegram Progress Tracker
// ─────────────────────────────────────────────────────────────────────────────

struct TelegramProgress {
    bot_token: String,
    chat_id: String,
    status_msg_id: Option<i64>,
    title: String,
    label: String,
    fmt: String,
    job_id: i64,
    last_update: Mutex<Option<Instant>>,
}

impl TelegramProgress {
    fn new(
        bot_token: String,
        chat_id: String,
        status_msg_id: &str,
        title: String,
        label: String,
        fmt: &str,
        job_id: i64,
    ) -> Self {
        let msg_id = status_msg_id.trim().parse::<i64>().ok();
        Self {
            bot_token,
            chat_id,
            status_msg_id: msg_id,
            title,
            label,
            fmt: fmt.to_uppercase(),
            job_id,
            last_update: Mutex::new(None),
        }
    }

    fn bar(percent: i32) -> String {
        let total = 10;
        let filled = ((percent as f64 / 10.0).round() as i32).clamp(0, total) as usize;
        format!("{}{}", "▰".repeat(filled), "▱".repeat(total as usize - filled))
    }

    async fn update(&self, client: &Client, stage_text: &str, percent: i32, force: bool) {
        let Some(msg_id) = self.status_msg_id else {
            return;
        };
        {
            let mut guard = self.last_update.lock().await;
            if !force {
                if let Some(last) = *guard {
                    if last.elapsed() < Duration::from_millis(1500) {
                        return;
                    }
                }
            }
            *guard = Some(Instant::now());
        }

        let bar_str = Self::bar(percent);
        let text = format!(
            "⏳ <b>در حال پردازش دانلود:</b>\n\n📖 <b>{}</b>\n🔖 <b>{}</b>\n📦 فرمت: <b>{}</b>\n\n{stage_text}\n<code>[{bar_str}]</code> <b>{percent}%</b>",
            html_escape(&self.title),
            html_escape(&self.label),
            self.fmt
        );

        let _ = client
            .post(format!(
                "https://api.telegram.org/bot{}/editMessageText",
                self.bot_token
            ))
            .json(&json!({
                "chat_id": self.chat_id,
                "message_id": msg_id,
                "text": text,
                "parse_mode": "HTML"
            }))
            .timeout(Duration::from_secs(5))
            .send()
            .await;
    }

    async fn send_error(&self, client: &Client, error_text: &str) {
        let mut keyboard = Vec::new();
        if self.job_id != 0 {
            keyboard.push(vec![json!({
                "text": "⚠️ گزارش خطا به ادمین",
                "callback_data": format!("rep_err:{}", self.job_id)
            })]);
        }
        keyboard.push(vec![json!({
            "text": "🏠 بازگشت به منوی اصلی",
            "callback_data": "p:home"
        })]);

        let reply_markup = json!({ "inline_keyboard": keyboard });

        if let Some(msg_id) = self.status_msg_id {
            if let Ok(res) = client
                .post(format!(
                    "https://api.telegram.org/bot{}/editMessageText",
                    self.bot_token
                ))
                .json(&json!({
                    "chat_id": self.chat_id,
                    "message_id": msg_id,
                    "text": error_text,
                    "parse_mode": "HTML",
                    "reply_markup": reply_markup
                }))
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                if res.status().is_success() {
                    return;
                }
            }
        }

        let _ = client
            .post(format!(
                "https://api.telegram.org/bot{}/sendMessage",
                self.bot_token
            ))
            .json(&json!({
                "chat_id": self.chat_id,
                "text": error_text,
                "parse_mode": "HTML",
                "reply_markup": reply_markup
            }))
            .timeout(Duration::from_secs(5))
            .send()
            .await;
    }

    async fn delete(&self, client: &Client) {
        let Some(msg_id) = self.status_msg_id else {
            return;
        };
        let _ = client
            .post(format!(
                "https://api.telegram.org/bot{}/deleteMessage",
                self.bot_token
            ))
            .json(&json!({
                "chat_id": self.chat_id,
                "message_id": msg_id
            }))
            .timeout(Duration::from_secs(5))
            .send()
            .await;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Parallel Image Downloader
// ─────────────────────────────────────────────────────────────────────────────

fn detect_image_ext(content: &[u8]) -> Option<&'static str> {
    if content.len() < 8 {
        return None;
    }
    if content.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(".jpg");
    }
    if content.starts_with(b"\x89PNG") {
        return Some(".png");
    }
    if content.starts_with(b"RIFF") && content[..content.len().min(16)].windows(4).any(|w| w == b"WEBP") {
        return Some(".webp");
    }
    if content.starts_with(b"GIF8") {
        return Some(".gif");
    }
    if content.len() >= 12 {
        let prefix = &content[..content.len().min(32)];
        if prefix.windows(8).any(|w| w == b"ftypavif" || w == b"ftypavis") {
            return Some(".avif");
        }
    }
    None
}

fn stitch_horizontal(image_bytes_list: &[Vec<u8>]) -> Option<Vec<u8>> {
    if image_bytes_list.is_empty() {
        return None;
    }
    let mut imgs: Vec<DynamicImage> = Vec::with_capacity(image_bytes_list.len());
    for b in image_bytes_list {
        match image::load_from_memory(b) {
            Ok(im) => imgs.push(im),
            Err(e) => {
                eprintln!("stitch_horizontal decode error: {e}");
                return image_bytes_list.first().cloned();
            }
        }
    }
    let total_w: u32 = imgs.iter().map(|im| im.width()).sum();
    let max_h: u32 = imgs.iter().map(|im| im.height()).max().unwrap_or(0);
    let mut canvas = RgbaImage::new(total_w, max_h);
    let mut cur_x = 0;
    for im in imgs {
        let rgba = im.to_rgba8();
        let _ = canvas.copy_from(&rgba, cur_x, 0);
        cur_x += rgba.width();
    }
    let mut out = Cursor::new(Vec::new());
    if DynamicImage::ImageRgba8(canvas)
        .write_to(&mut out, ImageFormat::Png)
        .is_ok()
    {
        Some(out.into_inner())
    } else {
        image_bytes_list.first().cloned()
    }
}

async fn fetch_image_bytes(client: &Client, url: &str, referer: &str) -> Option<Vec<u8>> {
    let mut current_url = url.to_string();
    let effective_referer = if url.contains("image-comic.pstatic.net") {
        "https://comic.naver.com"
    } else if url.contains("bzcdn.net") {
        "https://www.twbzmg.com/"
    } else {
        referer
    };
    for attempt in 0..3 {
        if let Ok(res) = client
            .get(&current_url)
            .header("User-Agent", USER_AGENT)
            .header("Referer", effective_referer)
            .header(
                "Accept",
                "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
            )
            .timeout(Duration::from_secs(12))
            .send()
            .await
        {
            if res.status().is_success() {
                if let Ok(bytes) = res.bytes().await {
                    if bytes.len() >= 32 {
                        return Some(bytes.to_vec());
                    }
                }
            }
        }
        if current_url.contains("hip-cf-1.s3imgs.top") {
            current_url = current_url.replace("hip-cf-1.s3imgs.top", "hip-tx-1.s3imgs.top");
        } else if current_url.contains("hip-tx-1.s3imgs.top") {
            current_url = current_url.replace("hip-tx-1.s3imgs.top", "hip-cf-1.s3imgs.top");
        }
        tokio::time::sleep(Duration::from_millis(200 * (attempt + 1))).await;
    }
    None
}

async fn download_single_image(
    client: &Client,
    sem: &Semaphore,
    item: PageItem,
    index: usize,
    output_dir: &Path,
    referer: &str,
) -> Option<PathBuf> {
    let _permit = sem.acquire().await.ok()?;
    match item {
        PageItem::Strips(part_urls) => {
            let mut strip_bytes = Vec::with_capacity(part_urls.len());
            for part_url in part_urls {
                let bytes = fetch_image_bytes(client, &part_url, referer).await?;
                strip_bytes.push(bytes);
            }
            let stitched = tokio::task::spawn_blocking(move || stitch_horizontal(&strip_bytes))
                .await
                .ok()??;
            let filepath = output_dir.join(format!("{index:04}.png"));
            fs::write(&filepath, stitched).ok()?;
            Some(filepath)
        }
        PageItem::Single(url) => {
            let bytes = fetch_image_bytes(client, &url, referer).await?;
            let ext = detect_image_ext(&bytes)?;
            let filepath = output_dir.join(format!("{index:04}{ext}"));
            fs::write(&filepath, bytes).ok()?;
            Some(filepath)
        }
    }
}

async fn download_all_images(
    page_urls: Vec<PageItem>,
    referer: &str,
    output_dir: &Path,
    progress: Arc<TelegramProgress>,
    client: &Client,
) -> Vec<PathBuf> {
    let sem = Arc::new(Semaphore::new(25));
    let total = page_urls.len();
    let completed = Arc::new(Mutex::new(0usize));
    let mut handles = Vec::with_capacity(total);

    for (idx, item) in page_urls.into_iter().enumerate() {
        let sem_c = Arc::clone(&sem);
        let comp_c = Arc::clone(&completed);
        let prog_c = Arc::clone(&progress);
        let client_c = client.clone();
        let out_dir = output_dir.to_path_buf();
        let ref_str = referer.to_string();

        handles.push(tokio::spawn(async move {
            let res =
                download_single_image(&client_c, &sem_c, item, idx + 1, &out_dir, &ref_str).await;
            let done = {
                let mut c = comp_c.lock().await;
                *c += 1;
                *c
            };
            let pct = 10 + ((done as f64 / total as f64) * 70.0) as i32;
            prog_c
                .update(
                    &client_c,
                    &format!("📥 در حال دریافت صفحات ({done} از {total}):"),
                    pct,
                    done == total,
                )
                .await;
            res
        }));
    }

    let mut files = Vec::new();
    for h in handles {
        if let Ok(Some(path)) = h.await {
            files.push(path);
        }
    }
    files
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Archive Packaging (CBZ / ZIP)
// ─────────────────────────────────────────────────────────────────────────────

fn create_archive(image_files: &[PathBuf], output_path: &Path) -> std::io::Result<u64> {
    let mut sorted = image_files.to_vec();
    sorted.sort_by_key(|p| p.file_name().map(|n| n.to_os_string()));

    let file = File::create(output_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    for f in sorted {
        if let Some(name) = f.file_name().and_then(|n| n.to_str()) {
            zip.start_file(name, options)?;
            let data = fs::read(&f)?;
            zip.write_all(&data)?;
        }
    }
    zip.finish()?;
    Ok(fs::metadata(output_path)?.len())
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Temporary Upload & Single-Message Telegram Delivery
// ─────────────────────────────────────────────────────────────────────────────

async fn upload_to_litterbox(client: &Client, file_path: &Path, expiry: &str) -> Option<String> {
    let mut options = vec![expiry];
    for e in ["12h", "24h", "1h", "72h"] {
        if e != expiry {
            options.push(e);
        }
    }
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("archive.zip")
        .to_string();

    for exp in options {
        let Ok(file) = tokio::fs::File::open(file_path).await else {
            return None;
        };
        let len = file.metadata().await.ok().map(|m| m.len()).unwrap_or(0);
        let stream = tokio_util::codec::FramedRead::new(file, tokio_util::codec::BytesCodec::new());
        let body = reqwest::Body::wrap_stream(stream);
        let Ok(part) = multipart::Part::stream_with_length(body, len)
            .file_name(file_name.clone())
            .mime_str("application/octet-stream")
        else {
            continue;
        };

        let form = multipart::Form::new()
            .text("reqtype", "fileupload")
            .text("time", exp.to_string())
            .part("fileToUpload", part);

        match client
            .post("https://litterbox.catbox.moe/resources/internals/api.php")
            .multipart(form)
            .timeout(Duration::from_secs(120))
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if status.is_success() && text.trim().starts_with("http") {
                    return Some(text.trim().to_string());
                }
                eprintln!(
                    "Litterbox non-200 ({exp}): {status} {}",
                    &text[..text.len().min(100)]
                );
            }
            Err(e) => eprintln!("Litterbox upload error ({exp}): {e}"),
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
async fn deliver_to_telegram(
    client: &Client,
    progress: &TelegramProgress,
    bot_token: &str,
    chat_id: &str,
    archive_path: &Path,
    title: &str,
    label: &str,
    archive_format: &str,
    delivery_mode: &str,
) -> (bool, Option<String>) {
    let file_size = fs::metadata(archive_path).map(|m| m.len()).unwrap_or(0);
    let size_mb = file_size as f64 / (1024.0 * 1024.0);
    let file_too_large = file_size as f64 > 49.5 * 1024.0 * 1024.0;
    let should_send_link = delivery_mode == "link_only" || file_too_large;
    let fmt_upper = archive_format.to_uppercase();
    let title_esc = html_escape(title);
    let label_esc = html_escape(label);

    if !should_send_link {
        progress
            .update(client, "🚀 در حال ارسال فایل به تلگرام...", 95, true)
            .await;
        let caption = format!(
            "🎉 <b>فایل آماده شد!</b>\n\n📖 <b>{title_esc}</b>\n🔖 <b>{label_esc}</b>\n📦 فرمت: <b>{fmt_upper}</b> ({size_mb:.1} MB)\n\n✨ <i>آماده مطالعه در نرم‌افزارهای کمیک‌خوان و گالری.</i>"
        );
        let mut keyboard = Vec::new();
        if progress.job_id != 0 {
            keyboard.push(vec![json!({
                "text": "⚠️ گزارش مشکل در فایل",
                "callback_data": format!("rep_err:{}", progress.job_id)
            })]);
        }
        keyboard.push(vec![json!({
            "text": "🏠 بازگشت به منوی اصلی",
            "callback_data": "p:home"
        })]);
        let reply_markup = json!({ "inline_keyboard": keyboard }).to_string();

        if let Ok(file) = tokio::fs::File::open(archive_path).await {
            let file_name = archive_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("chapter.zip")
                .to_string();
            let stream =
                tokio_util::codec::FramedRead::new(file, tokio_util::codec::BytesCodec::new());
            let body = reqwest::Body::wrap_stream(stream);
            if let Ok(part) = multipart::Part::stream_with_length(body, file_size)
                .file_name(file_name)
                .mime_str("application/octet-stream")
            {
                let form = multipart::Form::new()
                    .text("chat_id", chat_id.to_string())
                    .text("caption", caption)
                    .text("parse_mode", "HTML")
                    .text("reply_markup", reply_markup)
                    .part("document", part);

                match client
                    .post(format!(
                        "https://api.telegram.org/bot{bot_token}/sendDocument"
                    ))
                    .multipart(form)
                    .timeout(Duration::from_secs(180))
                    .send()
                    .await
                {
                    Ok(res) if res.status().is_success() => {
                        progress.delete(client).await;
                        return (true, None);
                    }
                    Ok(res) => {
                        eprintln!(
                            "Telegram upload failed: {}",
                            res.text().await.unwrap_or_default()
                        );
                    }
                    Err(e) => eprintln!("Direct file delivery exception: {e}"),
                }
            }
        }
    }

    progress
        .update(client, "🌐 در حال آماده‌سازی لینک مستقیم دانلود...", 92, true)
        .await;
    let Some(link_url) = upload_to_litterbox(client, archive_path, "12h").await else {
        return (false, Some("Failed to upload to temporary storage".to_string()));
    };

    let reason_text = if file_too_large {
        "به دلیل حجم بالای فایل (بیش از ۵۰ مگابایت)"
    } else {
        "طبق تنظیمات تحویل مستقیم"
    };

    let msg_text = format!(
        "🎉 <b>فایل آماده شد!</b>\n\n📖 <b>{title_esc}</b>\n🔖 <b>{label_esc}</b>\n📦 فرمت: <b>{fmt_upper}</b> ({size_mb:.1} MB)\n\nℹ️ {reason_text}، لینک دانلود مستقیم موقت برای شما ایجاد شد:\n\n🔗 <a href=\"{link_url}\"><b>دانلود مستقیم فایل {fmt_upper} (کلیک کنید)</b></a>\n\n⏳ <b>اعتبار لینک:</b> ۱۲ ساعت (پس از پایان مهلت به صورت خودکار حذف می‌شود)."
    );

    let mut rows = vec![vec![json!({
        "text": format!("📥 دانلود مستقیم ({fmt_upper} - {size_mb:.1}MB)"),
        "url": link_url
    })]];
    if progress.job_id != 0 {
        rows.push(vec![json!({
            "text": "⚠️ گزارش مشکل در فایل",
            "callback_data": format!("rep_err:{}", progress.job_id)
        })]);
    }
    rows.push(vec![json!({
        "text": "🏠 بازگشت به منوی اصلی",
        "callback_data": "p:home"
    })]);
    let keyboard = json!({ "inline_keyboard": rows });

    if let Some(msg_id) = progress.status_msg_id {
        if let Ok(res) = client
            .post(format!(
                "https://api.telegram.org/bot{bot_token}/editMessageText"
            ))
            .json(&json!({
                "chat_id": chat_id,
                "message_id": msg_id,
                "text": msg_text,
                "parse_mode": "HTML",
                "reply_markup": keyboard
            }))
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            if res.status().is_success() {
                return (true, None);
            }
        }
    }

    match client
        .post(format!(
            "https://api.telegram.org/bot{bot_token}/sendMessage"
        ))
        .json(&json!({
            "chat_id": chat_id,
            "text": msg_text,
            "parse_mode": "HTML",
            "reply_markup": keyboard
        }))
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(res) => (res.status().is_success(), None),
        Err(e) => (false, Some(e.to_string())),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Main Execution Entrypoint
// ─────────────────────────────────────────────────────────────────────────────

struct RunOutcome {
    status: &'static str,
    err_msg: Option<String>,
    file_size: Option<u64>,
    pages_count: usize,
    images_count: usize,
    exit_code: i32,
}

async fn run_pipeline(
    args: &Args,
    client: &Client,
    progress: Arc<TelegramProgress>,
    repo_root: &Path,
) -> RunOutcome {
    let title_esc = html_escape(&args.title);
    let label_esc = html_escape(&args.label);

    let Ok(tmp_dir) = tempfile::tempdir() else {
        return RunOutcome {
            status: "failed",
            err_msg: Some("Failed to create temp directory".to_string()),
            file_size: None,
            pages_count: 0,
            images_count: 0,
            exit_code: 1,
        };
    };

    let img_dir = tmp_dir.path().join("images");
    let _ = fs::create_dir_all(&img_dir);

    let chapter_url = normalize_chapter_url(&args.chapter_url);
    println!("[AmoDL] Target: {} ({}) - {}", args.title, args.label, chapter_url);

    progress
        .update(client, "🔍 در حال استخراج تصاویر چپتر...", 10, true)
        .await;

    let pages = match get_chapter_page_urls(client, &chapter_url, repo_root).await {
        Ok(p) => p,
        Err(AppError::ChapterLocked(cle)) => {
            let msg = if cle.is_empty() {
                "این چپتر قفل است (نیاز به سکه یا خرید دارد) و هنوز رایگان نشده است.".to_string()
            } else {
                cle
            };
            eprintln!("[AmoDL] Chapter locked error: {msg}");
            progress
                .send_error(
                    client,
                    &format!(
                        "🔒 <b>این چپتر قفل است:</b>\n\n📖 {title_esc}\n🔖 {label_esc}\n\n⚠️ {}\n<i>به محض آزادسازی رایگان در سورس، قابل دریافت خواهد بود.</i>",
                        html_escape(&msg)
                    ),
                )
                .await;
            return RunOutcome {
                status: "failed",
                err_msg: Some(msg),
                file_size: None,
                pages_count: 0,
                images_count: 0,
                exit_code: 1,
            };
        }
        Err(AppError::CloudflareBlocked(cbe)) => {
            eprintln!("[AmoDL] Cloudflare blocked error: {cbe}");
            progress
                .send_error(
                    client,
                    &format!(
                        "🛡 <b>سورس تحت محافظت ضدربات است:</b>\n\n📖 {title_esc}\n🔖 {label_esc}\n\n⚠️ {}\n<i>دانلود خودکار این سورس مسدود است. لطفاً از سورس‌های دیگر یا اپلیکیشن دسکتاپ استفاده فرمایید.</i>",
                        html_escape(&cbe)
                    ),
                )
                .await;
            return RunOutcome {
                status: "failed",
                err_msg: Some(cbe),
                file_size: None,
                pages_count: 0,
                images_count: 0,
                exit_code: 1,
            };
        }
        Err(AppError::Other(e)) => {
            eprintln!("[AmoDL] Extraction error: {e}");
            progress
                .send_error(
                    client,
                    &format!(
                        "❌ <b>خطای غیرمنتظره در پردازش دانلود:</b>\n<code>{}</code>\n\n📖 {title_esc}\n🔖 {label_esc}",
                        html_escape(&e.chars().take(200).collect::<String>())
                    ),
                )
                .await;
            return RunOutcome {
                status: "failed",
                err_msg: Some(e),
                file_size: None,
                pages_count: 0,
                images_count: 0,
                exit_code: 1,
            };
        }
    };

    let pages_count = pages.len();
    println!("[AmoDL] Discovered {pages_count} pages");
    if pages_count < 1 {
        let msg = "تصاویر این چپتر یافت نشد یا سورس توسط سیستم ضدربات (Cloudflare) مسدود شده است."
            .to_string();
        eprintln!("[AmoDL] Error: {msg}");
        progress
            .send_error(
                client,
                &format!("❌ <b>خطا در دریافت صفحات:</b> {msg}\n\n📖 {title_esc}\n🔖 {label_esc}"),
            )
            .await;
        return RunOutcome {
            status: "failed",
            err_msg: Some(msg),
            file_size: None,
            pages_count,
            images_count: 0,
            exit_code: 1,
        };
    }

    let images = download_all_images(
        pages,
        &chapter_url,
        &img_dir,
        Arc::clone(&progress),
        client,
    )
    .await;
    let images_count = images.len();
    println!("[AmoDL] Downloaded {images_count}/{pages_count} images");

    if images_count < 1 || (images_count as f64) < (pages_count as f64) * 0.5 {
        let msg = format!(
            "تعداد صفحات دریافت شده ({images_count} از {pages_count}) بسیار کم یا نامعتبر است."
        );
        eprintln!("[AmoDL] Error: {msg}");
        progress
            .send_error(
                client,
                &format!(
                    "❌ <b>خطا در دانلود تصاویر:</b> {}\n\n📖 {title_esc}\n🔖 {label_esc}",
                    html_escape(&msg)
                ),
            )
            .await;
        return RunOutcome {
            status: "failed",
            err_msg: Some(msg),
            file_size: None,
            pages_count,
            images_count,
            exit_code: 1,
        };
    }

    progress
        .update(
            client,
            &format!("📦 در حال بسته‌بندی فایل {}...", args.format.to_uppercase()),
            85,
            true,
        )
        .await;

    let clean_title: String = SANITIZE_RE
        .replace_all(&args.title, " ")
        .trim()
        .chars()
        .take(80)
        .collect();
    let clean_label: String = SANITIZE_RE
        .replace_all(&args.label, " ")
        .trim()
        .chars()
        .take(40)
        .collect();
    let clean_title = if clean_title.is_empty() { "Manga" } else { &clean_title };
    let clean_label = if clean_label.is_empty() { "Chapter" } else { &clean_label };

    let archive_name = format!("{clean_title} - {clean_label}.{}", args.format);
    let archive_path = tmp_dir.path().join(archive_name);

    let final_file_size = match create_archive(&images, &archive_path) {
        Ok(sz) => sz,
        Err(e) => {
            eprintln!("[AmoDL] Archive creation failed: {e}");
            return RunOutcome {
                status: "failed",
                err_msg: Some(format!("Archive creation error: {e}")),
                file_size: None,
                pages_count,
                images_count,
                exit_code: 1,
            };
        }
    };
    println!("[AmoDL] Archive created: {final_file_size} bytes");

    if final_file_size < 5 * 1024 || images_count < 1 {
        let msg = format!(
            "فایل فشرده حاصله ناقص یا بسیار کوچک ({final_file_size} بایت) است و قابل ارسال نمی‌باشد."
        );
        progress
            .send_error(
                client,
                &format!(
                    "❌ <b>خطا در اعتبارسنجی فایل:</b> {}\n\n📖 {title_esc}\n🔖 {label_esc}",
                    html_escape(&msg)
                ),
            )
            .await;
        return RunOutcome {
            status: "failed",
            err_msg: Some(msg),
            file_size: Some(final_file_size),
            pages_count,
            images_count,
            exit_code: 1,
        };
    }

    let (success, err) = deliver_to_telegram(
        client,
        &progress,
        &args.bot_token,
        &args.chat_id,
        &archive_path,
        &args.title,
        &args.label,
        &args.format,
        &args.delivery_mode,
    )
    .await;

    if success {
        println!("Delivery complete.");
        RunOutcome {
            status: "completed",
            err_msg: None,
            file_size: Some(final_file_size),
            pages_count,
            images_count,
            exit_code: 0,
        }
    } else {
        let e_msg = err.unwrap_or_else(|| "Delivery to Telegram failed".to_string());
        eprintln!("Delivery failed: {e_msg}");
        RunOutcome {
            status: "failed",
            err_msg: Some(e_msg),
            file_size: Some(final_file_size),
            pages_count,
            images_count,
            exit_code: 1,
        }
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if args.bot_token.is_empty() {
        eprintln!("Error: Missing TELEGRAM_BOT_TOKEN");
        std::process::exit(1);
    }

    let numeric_job_id = args.job_id.parse::<i64>().unwrap_or(0);
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let start_time = Instant::now();
    let progress = Arc::new(TelegramProgress::new(
        args.bot_token.clone(),
        args.chat_id.clone(),
        &args.status_msg_id,
        args.title.clone(),
        args.label.clone(),
        &args.format,
        numeric_job_id,
    ));

    let client = Client::builder()
        .pool_max_idle_per_host(35)
        .pool_idle_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to build HTTP client");

    let outcome = run_pipeline(&args, &client, Arc::clone(&progress), &repo_root).await;

    if !args.callback_url.is_empty() {
        let duration = start_time.elapsed().as_secs_f64();
        let payload = if outcome.status == "failed" {
            let final_log = build_final_log(
                numeric_job_id,
                &args.chat_id,
                &args.title,
                &args.label,
                &args.chapter_url,
                &args.format,
                &args.delivery_mode,
                outcome.status,
                duration,
                outcome.pages_count,
                outcome.images_count,
                outcome.file_size,
                outcome.err_msg.as_deref(),
                None,
            );
            json!({
                "job_id": numeric_job_id,
                "status": "failed",
                "error_msg": outcome.err_msg,
                "error_log": final_log,
                "file_size": outcome.file_size,
            })
        } else {
            json!({
                "job_id": numeric_job_id,
                "status": "completed",
                "file_size": outcome.file_size,
            })
        };

        if let Err(cb_err) = client
            .post(&args.callback_url)
            .header("X-Callback-Secret", &args.bot_token)
            .header("Authorization", format!("Bearer {}", args.bot_token))
            .json(&payload)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            eprintln!("Callback send error: {cb_err}");
        }
    }

    std::process::exit(outcome.exit_code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ts_reader_extraction_and_lock() {
        let html = r#"<script>ts_reader.run({"protected":false,"sources":[{"images":["https://cdn.example.com/1.jpg","/2.png","https://cdn.example.com/banner.jpg"]}]});</script>"#;
        let pages = extract_ts_reader_pages(html, "https://example.com/manga/ch-1").unwrap();
        assert_eq!(
            pages,
            vec![
                "https://cdn.example.com/1.jpg",
                "https://example.com/2.png"
            ]
        );

        let locked_html = r#"<script>ts_reader.run({"protected":true});</script>"#;
        assert!(matches!(
            extract_ts_reader_pages(locked_html, ""),
            Err(AppError::ChapterLocked(_))
        ));
    }

    #[test]
    fn test_detect_image_ext() {
        assert_eq!(detect_image_ext(b"\xff\xd8\xff\xe0\x00\x10JFIF"), Some(".jpg"));
        assert_eq!(detect_image_ext(b"\x89PNG\r\n\x1a\n"), Some(".png"));
        assert_eq!(detect_image_ext(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some(".webp"));
        assert_eq!(detect_image_ext(b"\x00\x00\x00\x1cftypavif\x00\x00"), Some(".avif"));
    }

    #[tokio::test]
    async fn test_extract_hipmh_pages_live() {
        let tmp = tempfile::tempdir().unwrap();
        let url = "https://reader.hipmh.top/chapter/bToxMDkzOC1jOjE4MDA4Nw-MTA5Mzg6MTI2LjAw";
        let pages = extract_hipmh_pages(url, tmp.path()).await;
        assert_eq!(pages.len(), 164);
        match &pages[0] {
            PageItem::Single(u) => assert!(u.starts_with("https://hip-cf-1.s3imgs.top/")),
            _ => panic!("Expected PageItem::Single"),
        }
    }

    #[tokio::test]
    async fn test_mgeko_chapter_resolution_live() {
        let client = Client::new();
        let tmp = tempfile::tempdir().unwrap();
        let fake_url = "https://www.mgeko.cc/manga/yooc-who-let-him-cultivate-immortality/chapter-35";
        let pages = get_chapter_page_urls(&client, fake_url, tmp.path()).await.unwrap();
        assert!(pages.len() >= 20, "Expected at least 20 pages from mgeko chapter 35, got {}", pages.len());
    }

    #[test]
    fn test_extract_valir_or_rsc_pages_nextjs() {
        let sample_html = r#"
            <script>:HL["/uploads/series/sample/001/cover-thumb.webp","image"]</script>
            <script>:HL["/uploads/series/sample/001/p-page1.webp","image"]</script>
            <script>:HL["/uploads/series/sample/001/p-page2.webp","image"]</script>
            <script>:HL["/uploads/series/sample/001/s-slice1.webp","image"]</script>
        "#;
        let pages = extract_valir_or_rsc_pages(sample_html, "https://example.com/chapter/1");
        assert_eq!(
            pages,
            vec![
                "https://example.com/uploads/series/sample/001/p-page1.webp",
                "https://example.com/uploads/series/sample/001/p-page2.webp",
            ]
        );

        if let Ok(kayn_html) = std::fs::read_to_string("/tmp/audit_dump/source-audit-results/kayn.html") {
            let kayn_pages = extract_valir_or_rsc_pages(&kayn_html, "https://kaynscans.com/series/comic/the-chaebol-familys-genius-monster/chapter/30");
            assert_eq!(kayn_pages.len(), 15);
            assert!(kayn_pages[0].contains("p-1fdac3fe-94ad-4c6a-a524-db9d5d7690a2.webp"));
        }

        if let Ok(elftoon_html) = std::fs::read_to_string("/tmp/audit_dump/source-audit-results/elftoon.html") {
            let elftoon_pages = extract_valir_or_rsc_pages(&elftoon_html, "https://elftoon.net/series/who-let-him-cultivate-immortality/chapter/31");
            assert_eq!(elftoon_pages.len(), 14);
            assert!(elftoon_pages[0].contains("p-067ebcef-f168-4757-9953-cda7cf71196e.webp"));
        }
    }

    #[tokio::test]
    async fn test_extract_naver_pages_live() {
        let client = Client::new();
        let tmp = tempfile::tempdir().unwrap();

        // 1. Desktop detail URL
        let desktop_url = "https://comic.naver.com/webtoon/detail?titleId=758037&no=1";
        let pages1 = get_chapter_page_urls(&client, desktop_url, tmp.path()).await.unwrap();
        assert!(pages1.len() >= 20, "Expected at least 20 pages from Naver desktop ch 1, got {}", pages1.len());

        // 2. Mobile detail URL
        let mobile_url = "https://m.comic.naver.com/webtoon/detail?titleId=758037&no=2";
        let pages2 = get_chapter_page_urls(&client, mobile_url, tmp.path()).await.unwrap();
        assert!(pages2.len() >= 20, "Expected at least 20 pages from Naver mobile ch 2, got {}", pages2.len());

        // 3. Detail URL with extra query params
        let param_url = "https://comic.naver.com/webtoon/detail?titleId=758037&no=1&week=mon&page=1";
        let pages3 = get_chapter_page_urls(&client, param_url, tmp.path()).await.unwrap();
        assert_eq!(pages1.len(), pages3.len(), "Expected same page count with extra query params");

        // 4. Series list URL fallback (no "no=" specified -> resolves to latest chapter)
        let list_url = "https://comic.naver.com/webtoon/list?titleId=758037";
        let pages_list = get_chapter_page_urls(&client, list_url, tmp.path()).await.unwrap();
        assert!(pages_list.len() >= 10, "Expected pages from latest episode via list URL fallback");

        // 5. Locked preview episode -> returns AppError::ChapterLocked
        let preview_url = "https://comic.naver.com/webtoon/detail?titleId=758037&no=270";
        let res_locked = get_chapter_page_urls(&client, preview_url, tmp.path()).await;
        match res_locked {
            Err(AppError::ChapterLocked(msg)) => assert!(msg.contains("قفل")),
            other => panic!("Expected AppError::ChapterLocked, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_extract_baoziv2_pages_live() {
        let client = Client::new();
        let tmp = tempfile::tempdir().unwrap();
        let baozi_url = "https://www.twbzmg.com/comic/chapter/dingjiqiyunqiaoqiaoxiulianqiannian-yuewenmanhua/0_271.html";
        let pages = get_chapter_page_urls(&client, baozi_url, tmp.path()).await.unwrap();
        assert!(pages.len() >= 10, "Expected at least 10 pages from Baozi/TwManga chapter, got {}", pages.len());
    }

    #[test]
    fn test_normalize_chapter_url() {
        assert_eq!(
            normalize_chapter_url("https://example.com/manga/chapter-1/chapter-1"),
            "https://example.com/manga/chapter-1"
        );
        assert_eq!(
            normalize_chapter_url("https://example.com/manga/chapter-1"),
            "https://example.com/manga/chapter-1"
        );
    }

    #[test]
    fn test_lock_re_detection() {
        let elftoon_locked = r#"
            <h3>Premium Chapter</h3>
            <p>Chapter 48 - 48</p>
            <p>Unlock to continue reading</p>
            <a>Sign in to Unlock</a>
            <p>Unlocking grants permanent access to this chapter</p>
        "#;
        assert!(LOCK_RE.is_match(elftoon_locked));
    }

    #[test]
    fn test_genztoons_uid_extraction() {
        let sample_html = r#"
            <div id="pages">
                <img src="/assets/images/placeholder.svg" uid="65c77e18cf0.avif" class="lazy w-full myImage">
                <img src="/assets/images/placeholder.svg" uid="65c77e1c5d4.avif" class="lazy w-full myImage">
                <img src="/assets/images/placeholder.svg" uid="65c77e1daeb.avif" class="lazy w-full myImage">
            </div>
        "#;
        let doc = Html::parse_document(sample_html);
        let uid_sel = Selector::parse("#pages img[uid], img.myImage[uid], div#pages img[uid]").unwrap();
        let mut urls = Vec::new();
        for el in doc.select(&uid_sel) {
            if let Some(uid) = el.value().attr("uid") {
                urls.push(format!("https://cdn.meowing.org/uploads/{uid}"));
            }
        }
        assert_eq!(urls.len(), 3);
        assert_eq!(urls[0], "https://cdn.meowing.org/uploads/65c77e18cf0.avif");
    }
}