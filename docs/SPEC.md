# SARIS — technical specification (shared contract)

## Layout
```
Cargo.toml
saris.toml.example      # example config (real saris.toml is user-provided)
src/                    # Rust backend (axum + tokio)
static/index.html       # upload form (embedded into binary via include_str!)
static/style.css        # embedded via include_str!
static/app.js           # embedded via include_str!
```

## HTTP API
| Method | Path | Description |
|---|---|---|
| GET | `/` | upload form (`static/index.html`) |
| GET | `/static/style.css`, `/static/app.js` | embedded assets |
| GET | `/api/config` | `{"subjects":["PE","EwEF",...],"max_file_size_mb":20,"allowed_extensions":["pdf","ipynb"],"doc_types":["raport","sprawozdanie"]}` — never exposes passwords |
| POST | `/api/upload` | `multipart/form-data` upload, see below |
| GET | `/admin` | server-rendered HTML browser of the archive (HTTP Basic Auth) |
| GET | `/admin/download/{subject}/{year}/{filename}` | file download (HTTP Basic Auth) |

### POST /api/upload — multipart fields
`s1_first`, `s1_last`, `s1_email`, `s2_first`, `s2_last`, `s2_email`, `s3_first`, `s3_last`, `s3_email`,
`subject`, `password`, `title`, `doc_type` (`raport` | `sprawozdanie`), `file` (the file part).

Validation (all errors collected, not just the first one):
- `subject` must be one of configured subjects; `password` must match that subject's password.
- At least student 1 must be complete (first, last, email). A student 2/3 entry that is partially filled is an error; a fully empty one is ignored. Every given email must be syntactically valid.
- `title` non-empty; `doc_type` one of allowed values.
- `file` present, non-empty, extension in `allowed_extensions` (case-insensitive), size ≤ `max_file_size_mb`. `.pdf` must start with `%PDF`; `.ipynb` must parse as JSON.

Success → `200 {"ok":true,"message":"...","stored_name":"...","email_sent":true|false}`
Error → `400` (or `413` too large, `500` server) `{"ok":false,"errors":["human-readable message in Polish", ...]}`

User-facing messages are in Polish; code and comments in English.

## Storage
`<storage_dir>/<subject>/<year>/<year>-<subject>-<title>-<s1_last>-<doc_type>-<YYYY-MM-DD_HH-MM-SS>.<ext>`
Components sanitized: Polish diacritics transliterated to ASCII, whitespace → `_`, only `[A-Za-z0-9_.-]` kept, title truncated to 60 chars. If the name exists, append `-2`, `-3`, ... Year = calendar year of upload (local time).

## Upload log (file, append, one line per attempt)
```
2026-10-20 11:42:03.123 | OK    | ip=1.2.3.4 | subject=PE | type=raport | title="..." | students=Jan Kowalski <jan@x.pl>; ... | file=/abs/path/to/file.pdf
2026-10-20 11:42:09.456 | ERROR | ip=1.2.3.4 | subject=PE | ... | file=- (original: name.pdf) | reason=Nieprawidłowe hasło; ...
```
Passwords are never logged. IP from socket address; if `trust_proxy = true`, first value of `X-Forwarded-For`.

## Config: saris.toml
```toml
[server]
bind = "0.0.0.0:3000"
storage_dir = "pliki"
log_file = "logs/saris.log"
max_file_size_mb = 20
allowed_extensions = ["pdf", "ipynb"]
trust_proxy = false

[admin]
username = "admin"
password = "change-me"

[smtp]
enabled = false
host = "smtp.example.com"
port = 587
username = ""
password = ""
from = "SARIS <saris@example.com>"

[[subjects]]
name = "PE"
password = "pe-secret"
# ... EwEF, PSM, PSW, LUKP
```
Config path: first CLI arg, else `saris.toml`.
