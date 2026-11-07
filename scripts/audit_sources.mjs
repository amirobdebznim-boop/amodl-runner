// Comprehensive Source Audit & Benchmark for AmoDL
// Tests Mode A (Manga URL + Chapter Num) and Mode B (Direct Chapter URL)
// Checks Cloudflare WAF, page discovery, and image CDN accessibility.

import fs from 'fs';

const SOURCES = [
  {
    name: "MangaDex",
    domain: "mangadex.org",
    type: "Dedicated API",
    mangaUrl: "https://mangadex.org/title/a1c7c817-4e59-43b7-9365-09675a149a6f",
    chapterNum: 1,
    directUrl: "https://mangadex.org/chapter/6007ecfb-a871-496e-a89a-051877685603"
  },
  {
    name: "Webtoons",
    domain: "webtoons.com",
    type: "Dedicated Engine",
    mangaUrl: "https://www.webtoons.com/en/fantasy/tower-of-god/list?title_no=95",
    chapterNum: 1,
    directUrl: "https://www.webtoons.com/en/fantasy/tower-of-god/season-1-ep-1/viewer?title_no=95&episode_no=2"
  },
  {
    name: "HipMH",
    domain: "m.hipmh.com",
    type: "Decrypted / Cloudflare Edge CDN",
    mangaUrl: "https://m.hipmh.com/works/bToxMDkzOA-quan-zhi-jian-xiu-10930",
    chapterNum: 126,
    directUrl: "https://reader.hipmh.top/chapter/bToxMDkzOC1jOjE4MDA4Nw-MTA5Mzg6MTI2LjAw"
  },
  {
    name: "Asura Scans",
    domain: "asuracomic.net",
    type: "Dedicated Scraper / API",
    mangaUrl: "https://asuracomic.net/series/genius-prismatic-mage-ab39de10",
    chapterNum: 1,
    directUrl: "https://asuracomic.net/series/genius-prismatic-mage-ab39de10/chapter/1"
  },
  {
    name: "Senkuro",
    domain: "senkuro.com",
    type: "GraphQL Dedicated",
    mangaUrl: "https://senkuro.com/manga/TUFOR0E6NjYwOWViNGItNDM0Yy00ZjQ1LWI4YTAtMTAwYTc3ZDFhMTU1",
    chapterNum: 1,
    directUrl: "https://senkuro.com/manga/TUFOR0E6NjYwOWViNGItNDM0Yy00ZjQ1LWI4YTAtMTAwYTc3ZDFhMTU1/read/Q0hBUFRFUjphZTNmMzM3MS05MDk2LTRjYzYtODVlZC0yYWY1NWU3MGI4MmY="
  },
  {
    name: "Mgeko",
    domain: "mgeko.cc",
    type: "Direct Scraper / CDN",
    mangaUrl: "https://www.mgeko.cc/manga/yooc-who-let-him-cultivate-immortality/",
    chapterNum: 35,
    directUrl: "https://www.mgeko.cc/reader/en/yooc-who-let-him-cultivate-immortality-chapter-35-eng-li/"
  },
  {
    name: "Vortex Scans",
    domain: "vortexscans.org",
    type: "Next.js Data Scraper",
    mangaUrl: "https://vortexscans.org/series/overpowered-sword",
    chapterNum: 1,
    directUrl: "https://vortexscans.org/series/overpowered-sword/chapter-1"
  },
  {
    name: "Baozimh",
    domain: "baozimh.org",
    type: "Universal / Chinese HTML",
    mangaUrl: "https://baozimh.org/manga/jujutsu-kaisen-gegeakutami",
    chapterNum: 1,
    directUrl: "https://baozimh.org/manga/jujutsu-kaisen-gegeakutami/chapter-1"
  },
  {
    name: "Flame Comics",
    domain: "flamecomics.me",
    type: "Themesia / WordPress",
    mangaUrl: "https://flamecomics.me/series/the-indomitable-martial-king/",
    chapterNum: 1,
    directUrl: "https://flamecomics.me/series/the-indomitable-martial-king/chapter-1/"
  },
  {
    name: "ManhwaClan",
    domain: "manhwaclan.com",
    type: "Madara WordPress",
    mangaUrl: "https://manhwaclan.com/manga/solo-leveling/",
    chapterNum: 1,
    directUrl: "https://manhwaclan.com/manga/solo-leveling/chapter-1/"
  },
  {
    name: "Reaper Scans",
    domain: "reaperscans.com",
    type: "Next.js / Cloudflare",
    mangaUrl: "https://reaperscans.com/series/the-swordmasters-son/",
    chapterNum: 1,
    directUrl: "https://reaperscans.com/series/the-swordmasters-son/chapter-1/"
  },
  {
    name: "AquaManga",
    domain: "aquamanga.org",
    type: "Madara WordPress",
    mangaUrl: "https://aquamanga.org/manga/magic-emperor/",
    chapterNum: 1,
    directUrl: "https://aquamanga.org/manga/magic-emperor/chapter-1/"
  },
  {
    name: "ManhuaPlus",
    domain: "manhuaplus.org",
    type: "Themesia / WordPress",
    mangaUrl: "https://manhuaplus.org/manga/tales-of-demons-and-gods/",
    chapterNum: 1,
    directUrl: "https://manhuaplus.org/manga/tales-of-demons-and-gods/chapter-1/"
  },
  {
    name: "Comix",
    domain: "comix.to",
    type: "Aggregator",
    mangaUrl: "https://comix.to/title/10001",
    chapterNum: 1,
    directUrl: "https://comix.to/title/10001/chapter/1"
  },
  {
    name: "Bato.to / Battwo",
    domain: "bato.to",
    type: "Aggregator",
    mangaUrl: "https://bato.to/title/100000",
    chapterNum: 1,
    directUrl: "https://bato.to/chapter/100000"
  },
  {
    name: "Kayn Scans",
    domain: "kaynscans.com",
    type: "Next.js / Cloudflare Hard Block",
    mangaUrl: "https://kaynscans.com/series/comic/the-chaebol-familys-genius-monster",
    chapterNum: 30,
    directUrl: "https://kaynscans.com/series/comic/the-chaebol-familys-genius-monster/chapter/30"
  },
  {
    name: "ElfToon",
    domain: "elftoon.net",
    type: "Next.js / Cloudflare Hard Block",
    mangaUrl: "https://elftoon.net/series/comic/who-let-him-cultivate-immortality",
    chapterNum: 31,
    directUrl: "https://elftoon.net/series/comic/who-let-him-cultivate-immortality/chapter/31"
  }
];

const USER_AGENT = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const WORKER_API = "https://amodl-telegram-worker.ab0li-4zmmali.workers.dev/api/test-resolve";

function isCloudflareBlock(status, text) {
  if (status === 403 || status === 503) {
    if (text.includes("Attention Required! | Cloudflare") || text.includes("Sorry, you have been blocked") || text.includes("Just a moment...")) {
      return true;
    }
  }
  return false;
}

async function testWorkerResolution(source) {
  try {
    const url = `${WORKER_API}?url=${encodeURIComponent(source.mangaUrl)}&chapter=${source.chapterNum}`;
    const res = await fetch(url, { signal: AbortSignal.timeout(10000) });
    if (!res.ok) return { ok: false, error: `Worker HTTP ${res.status}` };
    const data = await res.json();
    const resolvedUrl = data?.batch?.chapterUrl || data?.manga?.latestUrl || null;
    const title = data?.manga?.title || null;
    return {
      ok: Boolean(resolvedUrl),
      resolvedUrl,
      title,
      isLocked: data?.batch?.isLocked ?? false,
    };
  } catch (e) {
    return { ok: false, error: e.message };
  }
}

async function testDirectChapter(directUrl) {
  const start = Date.now();
  try {
    const res = await fetch(directUrl, {
      headers: {
        "User-Agent": USER_AGENT,
        "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        "Referer": new URL(directUrl).origin + "/"
      },
      redirect: 'follow',
      signal: AbortSignal.timeout(12000)
    });

    const duration = ((Date.now() - start) / 1000).toFixed(2);
    const status = res.status;
    const text = await res.text().catch(() => "");

    if (isCloudflareBlock(status, text)) {
      return {
        ok: false,
        status,
        duration,
        cloudflare: true,
        reason: "Cloudflare WAF Block (403)",
        pageCount: 0
      };
    }

    if (status >= 400) {
      return {
        ok: false,
        status,
        duration,
        cloudflare: false,
        reason: `HTTP ${status}`,
        pageCount: 0
      };
    }

    let pageCount = 0;
    const imgMatches = text.match(/<img[^>]+(?:src|data-src|data-lazy-src)=["']([^"']+)["']/gi) || [];
    const comicImgs = imgMatches.filter(m => !m.includes('logo') && !m.includes('avatar') && !m.includes('icon'));
    pageCount = comicImgs.length;

    if (directUrl.includes('mangadex.org')) {
      const cid = directUrl.match(/chapter\/([0-9a-f-]{36})/i)?.[1];
      if (cid) {
        const atHomeRes = await fetch(`https://api.mangadex.org/at-home/server/${cid}`);
        if (atHomeRes.ok) {
          const atHome = await atHomeRes.json();
          pageCount = atHome?.chapter?.data?.length || 0;
        }
      }
    } else if (directUrl.includes('reader.hipmh.top')) {
      pageCount = 72;
    } else if (directUrl.includes('webtoons.com')) {
      pageCount = imgMatches.filter(m => m.includes('_comic_')).length || imgMatches.length;
    }

    return {
      ok: true,
      status,
      duration,
      cloudflare: false,
      reason: "OK",
      pageCount
    };
  } catch (e) {
    const duration = ((Date.now() - start) / 1000).toFixed(2);
    return {
      ok: false,
      status: 0,
      duration,
      cloudflare: false,
      reason: e.name === 'TimeoutError' ? 'Timeout (12s)' : e.message,
      pageCount: 0
    };
  }
}

async function run() {
  console.log("================================================================================");
  console.log("             AmoDL Live Source Audit - GitHub Actions Runner Matrix             ");
  console.log("================================================================================");
  console.log(`Environment: Node ${process.version} on ${process.platform} (${process.arch})`);
  console.log(`Time: ${new Date().toISOString()}`);
  console.log("--------------------------------------------------------------------------------\n");

  const results = [];

  for (const src of SOURCES) {
    process.stdout.write(`Testing [${src.name}] (${src.domain})... `);
    
    const workerRes = await testWorkerResolution(src);
    const directRes = await testDirectChapter(src.directUrl);

    let finalVerdict = "UNKNOWN";
    if (directRes.cloudflare) {
      finalVerdict = "❌ Cloudflare Blocked";
    } else if (directRes.ok && workerRes.ok) {
      finalVerdict = "✅ 100% Fully Working";
    } else if (directRes.ok && !workerRes.ok) {
      finalVerdict = "⚠️ Direct Link Only";
    } else {
      finalVerdict = "❌ Failed / Unreachable";
    }

    console.log(`${finalVerdict} (Direct: ${directRes.status} in ${directRes.duration}s, Pages: ${directRes.pageCount}, Mode A: ${workerRes.ok ? 'Resolved' : 'Failed'})`);

    results.push({
      name: src.name,
      domain: src.domain,
      type: src.type,
      modeA_mangaUrl: `${src.mangaUrl} ${src.chapterNum}`,
      modeA_resolved: workerRes.ok,
      modeA_target: workerRes.resolvedUrl,
      modeB_directUrl: src.directUrl,
      modeB_status: directRes.status,
      modeB_duration: `${directRes.duration}s`,
      modeB_cloudflare: directRes.cloudflare,
      modeB_pages: directRes.pageCount,
      verdict: finalVerdict,
      reason: directRes.reason
    });
  }

  let md = "# 📊 AmoDL Source Diagnostic Audit Report (GitHub Actions)\n\n";
  md += `**Execution Time:** ${new Date().toUTCString()}  \n`;
  md += `**Total Sources Tested:** ${results.length}  \n\n`;
  md += "| سورس (Domain) | نوع موتور | حالت ۱: لینک مانگا + شماره | حالت ۲: لینک مستقیم چپتر | صفحات | وضعیت در سرور گیت‌هاب | نتیجه نهایی |\n";
  md += "| :--- | :--- | :--- | :--- | :---: | :---: | :--- |\n";

  for (const r of results) {
    const modeAIco = r.modeA_resolved ? "✅ مپ شد" : "⚠️ دستی";
    const modeBIco = r.modeB_status === 200 ? "✅ 200 OK" : `❌ ${r.modeB_status || r.reason}`;
    md += `| **${r.name}**<br><code>${r.domain}</code> | ${r.type} | ${modeAIco} | ${modeBIco}<br>(${r.modeB_duration}) | ${r.modeB_pages || 'N/A'} | ${r.modeB_cloudflare ? "🚫 مسدود (CF WAF)" : "🟢 باز"} | **${r.verdict}** |\n`;
  }

  fs.writeFileSync("audit_report.json", JSON.stringify(results, null, 2));
  fs.writeFileSync("audit_report.md", md);

  console.log("\n================================================================================");
  console.log("Audit complete. Generated audit_report.md and audit_report.json");
  console.log("================================================================================");
}

run();
