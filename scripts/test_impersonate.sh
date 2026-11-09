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
