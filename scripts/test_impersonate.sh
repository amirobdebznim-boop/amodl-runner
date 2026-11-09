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
  "Elftoon|https://elftoon.net/series/comic/permanently-one-hp-yet-im-still-invincible/chapter/48"
  "Elftoon Base|https://elftoon.net/series/permanently-one-hp-yet-im-still-invincible/chapter/48"
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

echo "=== Comparative Inspection for Elftoon ==="
# 1. Old test chapter (who-let-him-cultivate-immortality/chapter/31)
$IMP_CHROME -s -L -o /tmp/elftoon_immortal.html "https://elftoon.net/series/who-let-him-cultivate-immortality/chapter/31"
echo "Immortal ch31 images found:"
grep -E -o 'https?://[^"'\'' \(\)]+\.(webp|jpg|jpeg|png|avif)' /tmp/elftoon_immortal.html | head -n 10 || true
grep -E -o '(/[^\s"'\'']+\.(webp|jpg|jpeg|png|avif))' /tmp/elftoon_immortal.html | head -n 10 || true

# 2. Same series, Chapter 1
$IMP_CHROME -s -L -o /tmp/elftoon_ch1.html "https://elftoon.net/series/comic/permanently-one-hp-yet-im-still-invincible/chapter/1"
echo "HP Chapter 1 images found:"
grep -E -o 'https?://[^"'\'' \(\)]+\.(webp|jpg|jpeg|png|avif)' /tmp/elftoon_ch1.html | head -n 10 || true
grep -E -o '(/[^\s"'\'']+\.(webp|jpg|jpeg|png|avif))' /tmp/elftoon_ch1.html | head -n 10 || true

# 3. Check reader chunk
$IMP_CHROME -s -L -o /tmp/reader_chunk.js "https://elftoon.net/_next/static/chunks/40436-15a989522a9d3ba5.js"
echo "Reader chunk size: $(wc -c < /tmp/reader_chunk.js)"
grep -i -o '[^";]*api[^";]*' /tmp/reader_chunk.js | head -n 20 || true
grep -i -o '[^";]*fetch([^)]*)' /tmp/reader_chunk.js | head -n 20 || true

# 4. Search all chunks referenced in ch48 for API or image URLs
mkdir -p /tmp/chunks
for chunk in $(grep -o '/_next/static/chunks/[^"]*' /tmp/elftoon_ch48.html | sort -u | head -n 20); do
  $IMP_CHROME -s -L -o "/tmp/chunks/$(basename $chunk)" "https://elftoon.net$chunk"
done
echo "API endpoints across chunks:"
grep -rhoE '["'\''/](api/[a-zA-Z0-9_/-]+)' /tmp/chunks/ | sort -u || true
