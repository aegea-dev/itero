---
title: The browser
description: The built-in #browser opens, reads and clicks through pages in your own Chrome.
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
| `key` | presses `Enter`, `Escape`, `Tab`, `Backspace`, an arrow, `PageUp`, `PageDown`, `Home` or `End` | the page after it |

The page, in every `$output`: `url`, `title` and `mode`. `timeout`, in
seconds, is how long `open`, `wait`, `click`, `hover` and `type` wait (30
when not given).

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
