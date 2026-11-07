---
title: The browser
description: The built-in #browser opens, reads, clicks through, inspects and saves pages in your own Chrome.
---

`#browser` drives Chrome, the real one installed on the machine: it opens
a page, waits for it, reads it, finds things on it, clicks, hovers and
types, as a person does. A step does one of these; the steps of a run act
on the same page, one after another, in a Chrome window of the run's own.

```json
{ "steps": [
    { "#browser": { "args": { "open": "https://news.ycombinator.com/" } } },
    { "#browser": { "args": { "click": "new" } } },
    { "#browser": { "args": { "read": "text" } } },
    { "#ai": { "args": { "model": "unsloth/Qwen3.5-4B-GGUF",
                         "message": "The three newest stories, one line each:\n$output.text" } } },
    { "#notify": { "args": { "title": "New on HN", "text": "$output" } } }
] }
```

It needs Chrome (on Linux, Chromium also does):
[google.com/chrome](https://www.google.com/chrome/).

## Chrome, its window and its profile

The daemon starts Chrome when a `#browser` step needs it, and stops it
after five minutes with no run using it. It is your Chrome, with a
profile of its own, kept in `~/.local/state/itero/browser/`: your own
profile, its logins and tabs, are never touched.

- **In a window** where there is a screen (macOS; Linux with a desktop),
  **headless** where there is none (a server, a container). A step's
  `"headless": true` or `false` says which; the first `#browser` step of a
  run chooses for the whole run, and `$output.mode` says which it got.
- **Log in once.** A site you need to be logged into: open it in the
  window itero's Chrome shows, and log in there. The profile remembers
  it, as your own Chrome does. The windowed and headless profiles are
  apart: a login in one is not in the other.
- **Sites that check for a person**, such as Reddit, let a window through
  once you have passed their check in it; headless they do not. Use a
  window for them.

Chrome is driven over its DevTools Protocol, on a port on `127.0.0.1`.
While Chrome runs, any program on the machine can reach that port, and
with it the logged-in profile. On a machine you share with other users,
keep that in mind before logging in to anything there.

## What a step does

| Arg | Does | `$output` |
|---|---|---|
| `open` | goes to an `http` or `https` address and waits until it has loaded (or changed in place, as many sites do) and gone quiet | the page |
| `wait` | waits until a target is on the page | the page, `found: true` |
| `find` | looks for a target once, never waits | `found`, `count`, and `text` of the first |
| `read` | `"text"` (below) or `"html"`; with `in`, one part of the page | the page, and `text` or `html` |
| `click` | waits for a target, then clicks it with the mouse | the page after it |
| `hover` | moves the mouse over a target | the page |
| `type` | types a text into a field, `into` its target | the page |
| `key` | presses `Enter`, `Escape`, `Tab`, `Backspace`, an arrow, `PageUp`, `PageDown`, `Home` or `End` (`End` goes to the bottom, as on an endless feed) | the page after it |
| `scroll` | scrolls until a target is in view | the page |
| `screenshot` | a picture of the page to a `.png` or `.jpg` file: the view, the whole page with `"full": true`, or one element with `in` | the page, `file` |
| `save` | the page as it is, to a folder or one `.mhtml` file (below) | the page, and where it went |
| `inspect` | opens an address as `open` does, and gives the page's facts (below) | the page, and `inspect` |

The page, in every `$output`: `url`, `title` and `mode`; after a page
loads, `blocked` and `captcha` when there is one (see Blockers). `timeout`, in
seconds, is how long `open`, `wait`, `click`, `hover`, `type` and
`inspect` wait (30 when not given). A file's path is the workspace's, or
absolute; its folders are made.

`width` and `height`, beside any action, resize the run's window first,
as you would drag it: a page laid out for a phone, a screenshot of a given
size. Both or neither, in pixels from 200 to 8000.

```json
{ "#browser": { "args": { "type": "itero", "into": "Search" } } },
{ "#browser": { "args": { "key": "Enter" } } }
```

## Targets

A target is what a person sees: the name of a link, a button or a field,
as it is written on the page (`"Log in"`, `"Search"`). When nothing has
that name, it is the element showing that text. Or, to be exact, a CSS
selector: `{"css": "#price"}`.

When several match, the first on the page is used; `find` says how many.

A click is a real one: the target is scrolled into view, and the mouse
clicks where it lies. When something else covers it there (a cookie
banner, a dialog), the step fails and says what.

## The page as text

`read: "text"` gives the page as a person reads it, the same on every
site:

```
# Sipariş
Kargo yarın gelir.
[Çıkış]
[button: Gönder]
Ara [Ara: ___]
```

Text as it reads, a paragraph on one line; `# ` before a heading;
`[name]` a link, `[button: name]` a button, `[label: ___]` a field. The
names in brackets are what `click` takes. This is the text to give `#ai`
to get something out of a page: a price, a list, the first post that is
not pinned.

## New tabs

A click that opens a tab (a link that opens in a new one) is followed:
the tab opens in front in the run's window, the run's next steps act on
it, and `$output.new_tab` is `true`. The run's window and tabs are
closed when it ends.

## The JSON a page loads

Many sites load their content as JSON after the page itself (Reddit
among them), which is steadier to read than the page. `responses`, with
`open`, `wait`, `click`, `key` or `inspect`, keeps the JSON responses
the page loaded while the step ran whose address contains a text:

```json
{ "#browser": { "args": { "open": "https://shop.example/", "responses": "/api/products" } } }
```

`$output.responses` lists each one's `url`, `status` and `json`, in the
order they came. A response that is not JSON by its type is left out.

## Saving a page

`save` keeps the page as it is, after the steps before it:

- **One file**, a path ending in `.mhtml`: the page and everything on it,
  which Chrome opens offline. Stylesheets in it are Chrome's reading of
  them, not the files byte for byte.
- **A folder**, any other path: every file the page loaded, HTML, CSS,
  JavaScript, images and fonts, byte for byte as they came, under
  `<host>/<path>`. It is for reading and comparing: its pages still link
  to the live site, so it does not browse offline; MHTML does.
  `$output` gives `folder`, `files` (how many) and `failed` (the
  addresses Chrome had nothing for). Some sites mark their pages not to
  be kept, and Chrome may have dropped its copy: such a page is fetched
  again through the page, with its cookies, and `$output.fetched_again`
  names it.

## Inspecting a page

`inspect` opens an address and gives `$output.inspect`:

| Field | Is |
|---|---|
| `document` | the page's status, its request and response headers, and the addresses it was redirected from |
| `files` | every file it loaded: `url`, `type`, `status`, `size` in bytes, `ms`, `mime` |
| `cookies` | each cookie's name, domain, path, `http_only`, `secure`, `same_site`, `expires`, and `third_party` |
| `meta` | title, description, canonical address, language, Open Graph and Twitter tags, JSON-LD |
| `libraries` | jQuery, React, Vue, Angular, Next.js, Nuxt, Svelte, Bootstrap, Google Analytics and Google Tag Manager when found, with their versions where they say them |
| `tls` | protocol, issuer, subject and expiry of an `https` page's certificate |
| `timing` | `first_byte_ms`, `dom_ready_ms`, `load_ms`, `first_contentful_paint_ms`, `largest_contentful_paint_ms`, `layout_shift` |
| `console` | the errors and warnings the page logged |
| `site` | `robots`, `sitemap` and `llms`: whether the site has `robots.txt`, `sitemap.xml` and `llms.txt`, their text (the sitemap's number of addresses) |

Cookie values are left out, as they are often a login, and a step's
`$output` is kept in the workspace: `"cookie_values": true` adds them.
Other libraries are not guessed: `files` lists every script, for `#ai`
or you to read. `robots.txt`, `sitemap.xml` and `llms.txt` are fetched
plainly, without the browser's cookies.

## Blockers

After `open`, and after a click that loads a page, the step looks at
what is in the way of the page.

- **Chrome's own bars and prompts** never come: itero's Chrome has no
  translate bar, never offers to save a password, and lets no site ask to
  send notifications.
- **Cookie banners of common consent tools** are rejected, with a click
  on their own reject button, as you would: OneTrust, Cookiebot, Didomi,
  Usercentrics, Google's and Reddit's. The workspace's log says so
  (`browser.consent`, with the tool and the page). Sourcepoint's banner
  is never clicked, since its reject is each site's own, at times a paid
  subscription.
- **Any other cookie banner** on top of the page, and Sourcepoint's, is
  shown to you as a notification, and the step goes on:
  `$output.blocked` is `["consent"]`, since it may be in the way. Answer
  it once in the window; the profile keeps your answer.
- **A human check in the way**, a page that asks you to prove you are a
  person (Reddit's, Cloudflare's) or a reCAPTCHA or hCaptcha challenge
  over the page: in a window, itero brings the page to the front and
  tells you with a notification, and the step waits for you to solve it,
  for as long as the workspace lets a step run (`most` in its
  [`limits`](/reference/itero-config/#limits)), saying so in its progress
  each minute (`itero ws wf runs` shows it). Headless, nobody can
  solve it: the step fails at once, and you are told. Give the run's
  first `#browser` step `"headless": false`.
- **A check that sits in a form** (a reCAPTCHA, hCaptcha or Turnstile
  box beside a login or a contact form) is not in the way of reading the
  page: `$output.captcha` names it, and the step goes on.

## When a step fails

The step says why and on which page, and saves a screenshot of it as
`.itero/browser/<run>-<step>.png` in the workspace, named in the error.
`#forget` removes it with its run.

```
#browser: "Log in" is not on the page after 30 s, on "Welcome" (https://...);
screenshot: .itero/browser/0199...-0.3.png
```

`itero ws check` says every mistake in every `#browser` step at once:
two actions in one step, a `key` it does not know, `type` without `into`.
