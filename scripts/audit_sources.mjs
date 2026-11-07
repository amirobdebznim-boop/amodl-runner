import fs from 'fs';
import path from 'path';

const EXTENSIONS_DIR = path.resolve('resources/extensions');
const files = fs.readdirSync(EXTENSIONS_DIR).filter(f => f.endsWith('.pcext.json')).sort();

console.log(`================================================================================`);
console.log(`       AmoDL Full 62-Source Live Audit - GitHub Actions Azure Runner Matrix      `);
console.log(`================================================================================`);
console.log(`Found ${files.length} source manifest files.`);

const USER_AGENT = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

function isCloudflare(status, text) {
  if (status === 403 || status === 503) {
    const l = (text || '').toLowerCase();
    return l.includes('cloudflare') || l.includes('cf-browser-verification') || l.includes('turnstile') || l.includes('just a moment') || l.includes('attention required') || l.includes('sorry, you have been blocked');
  }
  return false;
}

async function testSource(file) {
  const filePath = path.join(EXTENSIONS_DIR, file);
  const data = JSON.parse(fs.readFileSync(filePath, 'utf8'));
  const id = data.id || file.replace('.pcext.json', '');
  const name = data.name || id;
  const baseUrl = data.baseUrl || '';
  const lang = data.lang || 'en';

  if (!baseUrl) {
    return { name, id, baseUrl, lang, status: 0, duration: '0.00s', cloudflare: false, verdict: '⚠️ فاقد دامنه', statusClass: 'warn' };
  }

  const start = Date.now();
  try {
    const res = await fetch(baseUrl, {
      headers: {
        "User-Agent": USER_AGENT,
        "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        "Referer": baseUrl.endsWith('/') ? baseUrl : baseUrl + '/'
      },
      redirect: 'follow',
      signal: AbortSignal.timeout(8000)
    });
    const duration = ((Date.now() - start) / 1000).toFixed(2) + 's';
    const status = res.status;
    const text = await res.text().catch(() => "");

    if (isCloudflare(status, text)) {
      return { name, id, baseUrl, lang, status, duration, cloudflare: true, verdict: '🔴 مسدود (CF WAF)', statusClass: 'blocked' };
    }
    if (status >= 200 && status < 400) {
      return { name, id, baseUrl, lang, status, duration, cloudflare: false, verdict: '🟢 ۱۰۰٪ در دسترس', statusClass: 'ok' };
    }
    return { name, id, baseUrl, lang, status, duration, cloudflare: false, verdict: `⚠️ خطا (${status})`, statusClass: 'warn' };
  } catch (err) {
    const duration = ((Date.now() - start) / 1000).toFixed(2) + 's';
    return { name, id, baseUrl, lang, status: 0, duration, cloudflare: false, verdict: `❌ تایم‌اوت / قطع`, statusClass: 'err' };
  }
}

async function run() {
  const results = [];
  const concurrency = 6;
  const queue = [...files];

  async function worker() {
    while (queue.length > 0) {
      const file = queue.shift();
      const res = await testSource(file);
      results.push(res);
      console.log(`[${String(results.length).padStart(2, ' ')}/${files.length}] ${res.name.padEnd(30)} -> ${res.verdict.padEnd(20)} (${res.duration})`);
    }
  }

  await Promise.all(Array.from({ length: concurrency }, () => worker()));

  // Sort by status: OK first, then Warn, Blocked, Err
  const order = { ok: 0, warn: 1, blocked: 2, err: 3 };
  results.sort((a, b) => (order[a.statusClass] ?? 9) - (order[b.statusClass] ?? 9) || a.name.localeCompare(b.name));

  fs.writeFileSync('audit_report.json', JSON.stringify(results, null, 2));

  const totalOk = results.filter(r => r.statusClass === 'ok').length;
  const totalBlocked = results.filter(r => r.statusClass === 'blocked').length;
  const totalWarn = results.filter(r => r.statusClass === 'warn').length;
  const totalErr = results.filter(r => r.statusClass === 'err').length;

  let md = `# 📊 گزارش جامع بررسی تمام ۶۲ سورس AmoDL در گیت‌هاب اکشن\n\n`;
  md += `**تاریخ اجرا:** ${new Date().toUTCString()}  \n`;
  md += `**کل سورس‌های تعریف‌شده در اکستنشن‌ها:** ${results.length}  \n`;
  md += `**🟢 فعال و آزاد (بدون مسدودی):** ${totalOk} | **🔴 مسدود توسط آنتی‌ربات دیتاسنتر:** ${totalBlocked} | **⚠️ وضعیت خاص / خطا:** ${totalWarn + totalErr}\n\n`;

  md += `| # | عنوان سورس | دامنه / شناسه | زبان | کد وضعیت | پینگ سرور | نتیجه نهایی |\n`;
  md += `| :-: | :--- | :--- | :-: | :-: | :-: | :--- |\n`;

  results.forEach((r, idx) => {
    md += `| ${idx + 1} | **${r.name}** | <code>${r.id}</code> | ${r.lang} | ${r.status || 'N/A'} | ${r.duration} | ${r.verdict} |\n`;
  });

  fs.writeFileSync('audit_report.md', md);
  console.log(`\nAudit complete. Generated audit_report.md and audit_report.json for ${results.length} sources.`);
}

run();
