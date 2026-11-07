#!/bin/bash
set -e

UA="Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
ACCEPT="text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"
LANG="en-US,en;q=0.9"

echo "================================================================================"
echo "          Testing 15 Sources Directly via CURL on GitHub Actions Runner         "
echo "================================================================================"

declare -a SOURCES=(
  "Flame Comics|https://flamecomics.xyz|https://flamecomics.xyz/series/2/0c9db8012fbd1257"
  "Demonic Scans|https://demonicscans.org|https://demonicscans.org/title/Lookism"
  "Magus Manga|https://magustoon.org|https://magustoon.org/series"
  "Rawkuma|https://rawkuma.net|https://rawkuma.net/manga"
  "GoDa Manga|https://m.g-mh.org|https://m.g-mh.org/hots/page/1"
  "Comix|https://comix.to|https://comix.to"
  "MangaFire|https://mangafire.to|https://mangafire.to/filter"
  "ManhuaUS|https://manhuaus.com|https://manhuaus.com/manga"
  "ManhwaTop|https://manhwatop.com|https://manhwatop.com/manga"
  "EZ Manga|https://ezmanga.org|https://ezmanga.org/series"
  "Fast Manhua|https://manhuafast.net|https://manhuafast.net/manga"
  "Lua Comic|https://luacomic.org|https://luacomic.org"
  "Luna Toons|https://lunatoons.net|https://lunatoons.net/?browse=1"
  "ManhuaPlus (.com)|https://manhuaplus.com|https://manhuaplus.com"
  "ManhuaFree|https://manhuafree.com|https://manhuafree.com"
)

OUT_FILE="test_15_results.md"
echo "# 🔍 نتیجه بررسی دقیق ۱۵ سورس با cURL روی سرور گیت‌هاب اکشن" > $OUT_FILE
echo "" >> $OUT_FILE
echo "| سورس | دامنه / آدرس تستی | کد پاسخ (cURL) | زمان پاسخ | عنوان صفحه / وضعیت |" >> $OUT_FILE
echo "| :--- | :--- | :---: | :---: | :--- |" >> $OUT_FILE

for item in "${SOURCES[@]}"; do
  IFS="|" read -r name base test_url <<< "$item"
  echo "Testing [$name] -> $test_url"
  
  # Fetch headers and timing
  RESP=$(curl -s -L -A "$UA" -H "Accept: $ACCEPT" -H "Accept-Language: $LANG" \
    --max-time 10 -w "\n%{http_code}\n%{time_total}" "$test_url" || true)
  
  HTTP_CODE=$(echo "$RESP" | tail -n 2 | head -n 1)
  TOTAL_TIME=$(echo "$RESP" | tail -n 1)
  BODY=$(echo "$RESP" | sed '$d' | sed '$d')
  
  TITLE=$(echo "$BODY" | grep -i -o '<title[^>]*>[^<]*</title>' | head -n 1 | sed -e 's/<[^>]*>//g' | tr -d '\n\r' || true)
  
  if [ "$HTTP_CODE" = "200" ]; then
    VERDICT="🟢 **200 OK (کاملاً باز و فعال)**"
  elif [ "$HTTP_CODE" = "403" ]; then
    if echo "$BODY" | grep -qi "cloudflare\|turnstile\|just a moment"; then
      VERDICT="🔴 **Cloudflare WAF (۴۰۳ مسدود دیتاسنتر)**"
    else
      VERDICT="⚠️ **403 Forbidden**"
    fi
  elif [ "$HTTP_CODE" = "301" ] || [ "$HTTP_CODE" = "302" ]; then
    VERDICT="🔄 **ریدایرکت ($HTTP_CODE)**"
  elif [ -z "$HTTP_CODE" ] || [ "$HTTP_CODE" = "000" ]; then
    VERDICT="❌ **تایم‌اوت / عدم اتصال**"
  else
    VERDICT="⚠️ **HTTP $HTTP_CODE**"
  fi
  
  [ -z "$TITLE" ] && TITLE="بدون تگ Title"
  # Truncate title
  TITLE_SHORT=$(echo "$TITLE" | cut -c 1-40)
  
  echo "  Result: $HTTP_CODE ($TOTAL_TIME s) - $VERDICT - $TITLE_SHORT"
  echo "| **$name** | <code>$test_url</code> | $HTTP_CODE | ${TOTAL_TIME}s | $VERDICT ($TITLE_SHORT) |" >> $OUT_FILE
done

echo ""
echo "Saved report to $OUT_FILE"
