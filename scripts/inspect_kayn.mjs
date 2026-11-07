async function inspect(url) {
  console.log(`\n=== Inspecting: ${url} ===`);
  const res = await fetch(url, {
    headers: {
      "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
      "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
    }
  });
  console.log("Status:", res.status);
  const text = await res.text();
  console.log("Length:", text.length);
  
  // Find all matches for uploads/ or images
  const allImgs = [...text.matchAll(/https?:\/\/[^"'\s\\]+\.(?:jpg|jpeg|png|webp|avif)[^"'\s\\]*/gi)].map(m => m[0]);
  console.log("All image links:", allImgs);

  // Look for Next.js data or props
  const nextData = text.match(/<script id="__NEXT_DATA__"[^>]*>([\s\S]*?)<\/script>/i);
  if (nextData) {
    console.log("__NEXT_DATA__ found! Length:", nextData[1].length);
  }

  // Look for self.__next_f
  const nextF = [...text.matchAll(/self\.__next_f\.push\(\[1,"([\s\S]*?)"\]\)/g)];
  console.log("self.__next_f pushes count:", nextF.length);
  for (const nf of nextF) {
    if (nf[1].includes('chapter') || nf[1].includes('.webp') || nf[1].includes('.jpg')) {
      console.log("Found chunk with image/chapter:", nf[1].slice(0, 300));
    }
  }
}

await inspect("https://kaynscans.com/series/comic/the-chaebol-familys-genius-monster/chapter/30");
await inspect("https://elftoon.net/series/comic/who-let-him-cultivate-immortality/chapter/31");
