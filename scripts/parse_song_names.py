import argparse
from dataclasses import dataclass
from datetime import date
from genericpath import isfile
import os
import re
from typing import Optional

import structlog

log = structlog.get_logger()

parser = argparse.ArgumentParser()
parser.add_argument("directory")
parser.add_argument("--db")

args = parser.parse_args()
directory = args.directory
database = args.db

log.info("checking directory", directory=directory)

date_regex = re.compile(r"(?P<title>.+) \((?P<full_date>(?P<day>\d?\d) (?P<month>\d?\d) (?P<year>\d\d))\)")

@dataclass
class ParsedFile:
    title: str
    date: date
    filename: str

def parse_filename(filename: str) -> ParsedFile:
    matches = date_regex.search(filename)

    if matches is None:
        raise Exception("no match")

    song_title = matches["title"]
    
    day = matches["day"]
    month = matches["month"]
    year = matches["year"]

    return ParsedFile(song_title, date(int(f"20{year}"), int(month), int(day)), filename)

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