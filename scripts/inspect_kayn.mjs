import fs from 'fs';

async function inspect(url, filename) {
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
  fs.writeFileSync(filename, text);

  // Flight test
  const flightRes = await fetch(url, {
    headers: {
      "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
      "RSC": "1"
    }
  });
  console.log("Flight status:", flightRes.status);
  const flightText = await flightRes.text();
  console.log("Flight length:", flightText.length);
  fs.writeFileSync("flight_" + filename, flightText);
}

await inspect("https://kaynscans.com/series/comic/the-chaebol-familys-genius-monster/chapter/30", "kayn.html");
await inspect("https://elftoon.net/series/comic/who-let-him-cultivate-immortality/chapter/31", "elftoon.html");
