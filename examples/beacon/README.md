# beacon

A beacon: health checks on a schedule, and a notification only when
something changes. Every minute each site in the sites workflow's
settings is asked; every 10 minutes the disk's free space is checked.
When a site goes down, comes back, or the disk runs low, you hear about
it once.

Run `itero daemon` in this folder and leave it running. The first check
tells you what it watches; after that, only changes. Your own sites:
  itero config sites '{"sites": ["https://example.com"]}'
(the sites workflow's settings in itero.config.json; the disk
workflow's have its min_free). Your own
checks: an action that prints {"status": {"<what>": "<how it is>"}},
followed by #changed and #notify. The actions need curl, jq, df and awk.
