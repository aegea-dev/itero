# reddit-top

A subreddit's top post, opened in Chrome, its discussion read and summed
up by a model on this machine, as a notification.

```sh
itero ws emit subreddit.asked '{"url": "https://www.reddit.com/r/rust/"}'
```

1. `#browser` opens the subreddit and reads it as text.
2. `#ai` names the first post that is not pinned (`fields: {title}`).
3. `#browser` clicks that title, as a person would, and reads the
   discussion.
4. `#ai` sums it up; `#notify` shows it.

It needs Chrome and itero-ai with a model
([itero-ai](https://docs.itero.sh/ai/)).

Reddit checks that a person is there: the first time, it may show a
"Prove your humanity" page in the Chrome window itero opened. Solve it
once in that window, and the profile remembers it.
