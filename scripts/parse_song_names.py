import argparse
import os

import structlog

from util import parse_filename, ParsedFile

log = structlog.get_logger()

parser = argparse.ArgumentParser()
parser.add_argument("directory")
parser.add_argument("--db")

args = parser.parse_args()
directory = args.directory
database = args.db

log.info("checking directory", directory=directory)



songs = []

for file in os.listdir(directory):
    file_path = f"{directory}/{file}"
    if not os.path.isfile(file_path):
        log.debug("skipping as this is not a file", filename=file_path)
        continue

    log.info("inspecting", file=file)

    try:
        parsed = parse_filename(file)
        log.info("parsed file", parsed=parsed)
        songs.append(parsed)
    except Exception as e:
        log.warn("failed to parse filename", filename=file, error=e)

def format_command(parsed: ParsedFile) -> str:
    path = f"{directory}/{parsed.filename}"
    sung_date = f"{parsed.date.day:02}/{parsed.date.month:02}/{parsed.date.year}"
    return f"RUST_LOG=\"trace,symphonia=warn\" ../target/release/process_cli upload --title \"{parsed.title}\" --singer-id 2 --db \"{database}\" --sung-at \"{sung_date}\"  \"{path}\""

for song in songs:
    print(format_command(song))