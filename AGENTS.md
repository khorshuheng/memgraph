# Agent instructions

## Comments

Do not add comments unless explicitly asked to. This includes Rust `//` and
`///` lines, SQL `--` lines, and `#` comments in the Makefile, TOML configs and
scripts. Doc comments count as comments.

The Makefile's trailing `## ...` annotations on targets are the exception:
`make help` parses them as help text, so leave them in place.
