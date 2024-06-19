# Plink
A mediocre version of shazam designed specifically for identifying what song neuro is singing.

## Setup
1. read through [setup.md](setup.md) to get the database up and running
2. enter `/process_cli` use `cargo run -r -- upload-bulk --shell-script <script_path> --db <url> <directory>`
    1. This goes through every file in the directory, runs the provided shell script on it by calling `sh <script_path> "file_name"`
        1. If writing your own shell script, then check out [the default](scripts/single_wrapper.sh)
        2. The `singer_id` corresponds to an entry in the `singers` table

> [!warning]
> If you have an index setup inserting each song will take a *really* long time

## Matching
1. get any sample of a single song (can be full or partial) and pass it through
2. enter `/process_cli` use `cargo run -r -- discover --db <url> <file_path>`
    1. Other config options can be found in the command help
3. Will output a list of potential matches

> [!note]
> You can pass the `--json` flag to `discover` to get a json-formatted output
