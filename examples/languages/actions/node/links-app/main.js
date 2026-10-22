// The title and the first 20 links of the page at $url, with cheerio.
import * as cheerio from "cheerio";

const url = process.env.url;
const res = await fetch(url, { signal: AbortSignal.timeout(30_000) });
if (!res.ok) {
  console.error(`${url}: ${res.status} ${res.statusText}`);
  process.exit(1);
}
const $ = cheerio.load(await res.text());
const links = [
  ...new Set(
    $("a[href]")
      .map((_, a) => new URL($(a).attr("href"), url).href)
      .get()
      .filter((href) => href.startsWith("http")),
  ),
].slice(0, 20);
console.log(JSON.stringify({ title: $("title").first().text().trim(), links }));
