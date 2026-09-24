# H3 worker boundary

`app.py` is the only Modal deployment boundary. The checkpoint and its license/source must be pinned before enabling GPU generation. The desktop app must never call a MiniMax API or scrape the Modal web UI.
