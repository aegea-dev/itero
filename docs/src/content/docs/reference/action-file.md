---
title: Action files
description: Every key of an action's action.json.
---

An action of your own is a folder in an actions folder, holding an
`action.json` and its program. An unknown key is an error naming the file.
[Actions](/guides/actions/#your-own-actions) explains how a job reaches the
program.

```json
{
  "name": "photos",
  "version": 2,
  "type": "python@3.12",
  "requirements": "requirements.txt",
  "limits": { "timeout": "10m", "heartbeat": "1m" },
  "environment": { "PHOTOS_API": "https://photos.example.com" },
  "targets": {
    "resize": { "module": "photos", "callable": "resize" },
    "tag": { "module": "photos", "callable": "tag" }
  }
}
```

A workflow names a target `<name>.<target>@<version>`: `photos.resize@2`.

| Key | Is | |
|---|---|---|
| `name` | the folder's name; no `.` or `@`, not a built-in's | needed |
| `version` | a whole number from 1 | needed |
| `type` | `executable`, `python@<version>`, `bun@<version>` or `container` | needed |
| `image` | `container`: the image its program runs in, like `python:3.13-alpine` | with `container` |
| `targets` | named entry points, lowercase letters, digits and `-` | needed |
| `description` | text for people | |
| `environment` | variables the program gets; values may hold `${env.NAME}` [secrets](/guides/secrets/) | |
| `limits.timeout` | the most a job may take (60 seconds unless given); for `container`, from when its image is here | |
| `limits.prepare` | `container`: how long getting its image may take (10 minutes unless given); past it, the job runs again | |
| `limits.heartbeat` | the job's lease, renewed by touching `$ITERO_HEARTBEAT`; not longer than `timeout`; not for `container` yet | |
| `working_directory` | where the program runs, relative to the action's folder | |
| `home` | Python: the virtual environment's folder (`.venv`) | |
| `requirements` | Python: the requirements file | |

## A target

| `type` | Keys |
|---|---|
| `executable` | `executable`, and `arguments` (a list) |
| `python@` | `module` and `callable` |
| `bun@` | `file` and `export` |
| `container` | `executable`, and `arguments` (a list), inside the image; the action's folder is `/action` |
