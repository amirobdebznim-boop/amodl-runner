#!/usr/bin/env bash
set -e

echo "=== Installing curl-impersonate (Chrome 116 TLS & HTTP/2 impersonation) ==="
mkdir -p /tmp/curl-imp
curl -sL https://github.com/lwthiker/curl-impersonate/releases/download/v0.6.1/curl-impersonate-v0.6.1.x86_64-linux-gnu.tar.gz | tar -xz -C /tmp/curl-imp

IMP_CHROME="/tmp/curl-imp/curl_chrome116"
chmod +x /tmp/curl-imp/*

echo "curl-impersonate installed successfully."
$IMP_CHROME --version | head -n 1

TARGETS=(
  "ManhuaUS|https://manhuaus.com/manga"
  "Fast Manhua|https://manhuafast.net/manga"
  "Lua Comic|https://luacomic.org"
  "Luna Toons|https://lunatoons.net/?browse=1"
  "MangaBuff|https://mangabuff.ru"
  "ReManga|https://remanga.org/api/v3/titles/popular"
  "Comix|https://comix.to"
  "MangaFire|https://mangafire.to"
  "GenzToons Ch41|https://genztoons.org/series/the-absolute-being-s-modern-life/chapter-41"
  "GenzToons Ch1|https://genztoons.org/series/the-absolute-being-s-modern-life/chapter-1"
  "GenzToons Series|https://genztoons.org/series/the-absolute-being-s-modern-life/"
  "DivaScans Ch13|https://divascans.org/series/comic/supercritical-hell/chapter/13"
  "DivaScans Home|https://divascans.org"
  "EZManga Series|https://ezmanga.org/series/roxana-the-way-to-protect-the-female-leads-older-brother"
  "EZManga Chapter|https://ezmanga.org/series/roxana-the-way-to-protect-the-female-leads-older-brother/chapter-1"
  "Toonz Series|https://toonz.to/manhwa/peerless-madam-healer"
  "Toonz Chapter|https://toonz.to/manhwa/peerless-madam-healer/chapter-1"
)

echo "# 🛡️ نتایج تست نفوذ و عبور از کلودفلر با TLS Impersonation (Chrome 116)" > impersonate_results.md
echo "" >> impersonate_results.md
echo "| سورس | آدرس هدف | استاندارد cURL (قبل) | با curl_chrome116 (بعد) | نتیجه نهایی |" >> impersonate_results.md
echo "| :--- | :--- | :---: | :---: | :--- |" >> impersonate_results.md

for item in "${TARGETS[@]}"; do
  IFS="|" read -r name url <<< "$item"
  echo "Testing [$name] -> $url"

  # Standard curl test
  STD_RES=$(curl -s -L -o /tmp/std.html -w "%{http_code} (%{time_total}s)" --max-time 10 -A "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36" "$url" || echo "000 (timeout)")
  
  # Chrome 116 Impersonation test
  IMP_RES=$($IMP_CHROME -s -L -o /tmp/imp.html -w "%{http_code} (%{time_total}s)" --max-time 10 "$url" || echo "000 (timeout)")
  IMP_CODE=$(echo "$IMP_RES" | awk '{print $1}')

  TITLE=$(grep -i -o '<title[^>]*>[^<]*</title>' /tmp/imp.html 2>/dev/null | head -n 1 | sed -e 's/<[^>]*>//g' | tr -d '\n\r' | cut -c 1-40 || echo "")

  VERDICT="⚠️ نامشخص"
  if [ "$IMP_CODE" == "200" ]; then
    VERDICT="🟢 **موفق (۲۰۰ OK بدون چالش)**"
  elif [ "$IMP_CODE" == "403" ]; then
    if grep -iq "cloudflare" /tmp/imp.html 2>/dev/null || grep -iq "turnstile" /tmp/imp.html 2>/dev/null; then
      VERDICT="🔴 **همچنان مسدود (کپچای تعاملی Turnstile)**"
    else
      VERDICT="⚠️ **۴۰۳ سرور (احتمالاً Geo-block)**"
    fi
  else
    VERDICT="⚠️ **HTTP $IMP_CODE**"
  fi

  echo "| **$name** | <code>$url</code> | $STD_RES | **$IMP_RES** | $VERDICT ($TITLE) |" >> impersonate_results.md
  echo "  Result: Standard=$STD_RES vs Chrome116=$IMP_RES -> $VERDICT"
done

echo "" >> impersonate_results.md
cat impersonate_results.md

echo "=== Detailed Inspection for GenzToons ==="
$IMP_CHROME -s -L -o /tmp/genz_ch41.html "https://genztoons.org/chapter/64c85111b46-65c77e18cf0/"
echo "Ch41 HTML bytes: $(wc -c < /tmp/genz_ch41.html)"

$IMP_CHROME -s -L -o /tmp/genz_ch1.html "https://genztoons.org/chapter/64c85111b46-64c85c550a5/"
echo "Ch1 HTML bytes: $(wc -c < /tmp/genz_ch1.html)"

$IMP_CHROME -s -L -o /tmp/genz_ch45.html "https://genztoons.org/chapter/64c85111b46-65ed5731a30/"
echo "Ch45 HTML bytes: $(wc -c < /tmp/genz_ch45.html)"

echo "--- Ch41 Title & Meta ---"
grep -i -o '<title[^>]*>[^<]*</title>' /tmp/genz_ch41.html || true

echo "--- Ch41 All Non-Icon IMG tags ---"
grep -i -o '<img[^>]*>' /tmp/genz_ch41.html | grep -v "iconify" | head -n 35 || true

echo "--- Ch41 All image-like URLs ---"
grep -E -o 'https?://[^"'\'' ]+\.(webp|jpg|jpeg|png|avif)' /tmp/genz_ch41.html | head -n 35 || true
grep -E -o 'https?://wsrv\.nl/\?url=[^"'\'' \)]+' /tmp/genz_ch41.html | head -n 35 || true
grep -E -o 'https?://[a-zA-Z0-9.-]*meowing\.org/[^"'\'' \)]+' /tmp/genz_ch41.html | head -n 35 || true

echo "--- Ch41 Scripts ---"
grep -i -o '<script[^>]*>[^<]*</script>' /tmp/genz_ch41.html | head -n 15 || true

echo "--- Ch41 Lock / Coin / Turnstile indicators ---"
grep -i -E '(locked|premium|coin|turnstile|cloudflare)' /tmp/genz_ch41.html | head -n 10 || true

cp /tmp/genz_ch41.html ./genz_ch41.html
cp /tmp/genz_ch1.html ./genz_ch1.html
cp /tmp/genz_ch45.html ./genz_ch45.html

curl -sL "https://divascans.org/series/comic/supercritical-hell/chapter/13" -H "User-Agent: Mozilla/5.0" > diva_ch13.html

$IMP_CHROME -sL "https://kaynscans.com/series/comic/the-strongest-level-0-completely-unrivaled-with-analysis-skills/chapter/67" > kayn_ch67.html

echo "=== Detailed Inspection for EZManga ==="
$IMP_CHROME -s -L -o ./ezmanga_series.html "https://ezmanga.org/series/roxana-the-way-to-protect-the-female-leads-older-brother"
echo "EZManga Series bytes: $(wc -c < ./ezmanga_series.html)"
grep -i -o '<title[^>]*>[^<]*</title>' ./ezmanga_series.html || true
echo "--- EZManga Series Chapter Links ---"
grep -i -o '<a[^>]*href=[^>]*>' ./ezmanga_series.html | grep -i 'chapter' | head -n 30 || true

$IMP_CHROME -s -L -o ./ezmanga_ch1.html "https://ezmanga.org/series/roxana-the-way-to-protect-the-female-leads-older-brother/chapter-1"
echo "EZManga Ch1 bytes: $(wc -c < ./ezmanga_ch1.html)"
grep -i -o '<title[^>]*>[^<]*</title>' ./ezmanga_ch1.html || true
echo "--- EZManga Ch1 Images ---"
grep -i -o '<img[^>]*>' ./ezmanga_ch1.html | head -n 30 || true

echo "=== Detailed Inspection for Toonz ==="
$IMP_CHROME -s -L -o ./toonz_series.html "https://toonz.to/manhwa/peerless-madam-healer"
echo "Toonz Series bytes: $(wc -c < ./toonz_series.html)"
grep -i -o '<title[^>]*>[^<]*</title>' ./toonz_series.html || true

echo "--- Testing Toonz API endpoints and sister domains ---"
$IMP_CHROME -s -L -o ./toonz_api_genres.json "https://toonz.to/api/genres"
echo "Toonz API genres bytes: $(wc -c < ./toonz_api_genres.json)"
head -c 200 ./toonz_api_genres.json; echo ""

$IMP_CHROME -s -L -H "rsc: 1" -o ./toonz_rsc.txt "https://toonz.to/manhwa/peerless-madam-healer"
echo "Toonz with RSC header bytes: $(wc -c < ./toonz_rsc.txt)"
head -c 300 ./toonz_rsc.txt; echo ""

$IMP_CHROME -s -L -o ./toonz_comics.html "https://toonz.to/comics"
echo "Toonz /comics bytes: $(wc -c < ./toonz_comics.html)"
grep -i -o '<title[^>]*>[^<]*</title>' ./toonz_comics.html || true

$IMP_CHROME -s -L -o ./toonz_search.html "https://toonz.to/search?q=peerless"
echo "Toonz /search?q=peerless bytes: $(wc -c < ./toonz_search.html)"
grep -i -o '<title[^>]*>[^<]*</title>' ./toonz_search.html || true

$IMP_CHROME -s -L -H "rsc: 1" -o ./toonz_search_rsc.txt "https://toonz.to/search?q=peerless"
echo "Toonz /search with RSC bytes: $(wc -c < ./toonz_search_rsc.txt)"
head -c 300 ./toonz_search_rsc.txt; echo ""

echo "--- Testing Toonz Chapter 1 RSC ---"
$IMP_CHROME -s -L -H "rsc: 1" -o ./toonz_chapter1_rsc.txt "https://toonz.to/manhwa/peerless-madam-healer/chapter/1"
echo "Toonz Chapter 1 RSC bytes: $(wc -c < ./toonz_chapter1_rsc.txt)"
head -c 500 ./toonz_chapter1_rsc.txt; echo ""

$IMP_CHROME -s -L -o ./toonz_chapter1_html.html "https://toonz.to/manhwa/peerless-madam-healer/chapter/1"
echo "Toonz Chapter 1 HTML bytes: $(wc -c < ./toonz_chapter1_html.html)"
grep -i -o '<title[^>]*>[^<]*</title>' ./toonz_chapter1_html.html || true

echo "=== Python curl_cffi check ==="
python3 -m pip install --quiet curl_cffi
python3 -c "
import sys
from curl_cffi import requests
for u in ['https://ezmanga.org/series/roxana-the-way-to-protect-the-female-leads-older-brother', 'https://toonz.to/manhwa/peerless-madam-healer']:
    for imp in ['chrome120', 'chrome124', 'safari17_0']:
        try:
            r = requests.get(u, impersonate=imp, timeout=15)
            is_cf = 'Just a moment' in r.text or 'Attention Required' in r.text
            t = r.text[r.text.find('<title>'):r.text.find('</title>')+8] if '<title>' in r.text else 'no-title'
            print(f'cffi [{imp}] {u} -> {r.status_code}, cf={is_cf}, title={t}, len={len(r.text)}')
            if not is_cf and r.status_code == 200:
                fn = 'ezmanga_cffi.html' if 'ezmanga' in u else 'toonz_cffi.html'
                with open(fn, 'w') as f:
                    f.write(r.text)
        except Exception as e:
            print(f'cffi err {imp} {u}: {e}')
"
