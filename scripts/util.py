"""Utilities for parsing file names from [the google drive of songs](https://drive.google.com/drive/folders/118gr4QuaGQGKfJ0X8VBCytvPjdzPayPY)"""

from dataclasses import dataclass
from datetime import date
import re
from typing import Optional


v3_date_regex = re.compile(
    r"(?P<title>.+) \((?P<full_date>(?P<day>\d?\d) (?P<month>\d?\d) (?P<year>\d\d))\)"
)
v1_date_regex = re.compile(
    r"\[(?P<full_date>(?P<month>\d?\d)(?:-|／)(?P<day>\d?\d)(?:-|／)(?P<year>\d\d))\] (?P<title>.+)(?: \[\d+\])?\..+"
)
evil_date_regex = re.compile(
    r"^(?P<title>[^\(\)]+)(?: \((?P<full_date>(?P<day>\d?\d) (?P<month>\d?\d) (?P<year>\d\d))\))?\..+$"
)
duet_date_regex = v3_date_regex


@dataclass
class ParsedFile:
    title: str
    date: Optional[date]
    filename: str


def evil_preprocess(filename: str) -> str:
    return filename.replace(" (evil)", "")


def parse_filename(filename: str) -> ParsedFile:
    regexes = [
        (evil_preprocess, evil_date_regex),
        (None, v3_date_regex),
        (None, v1_date_regex),
        (None, duet_date_regex),
    ]

    for preprocess, regex in regexes:
        iter_fname = filename
        try:
            if preprocess:
                iter_fname = preprocess(iter_fname)

            parsed = apply_regex(iter_fname, regex)
            return parsed
        except Exception as _:
            continue

    raise Exception("no matches from any provider")


def apply_regex(filename: str, regex: re.Pattern[str]) -> ParsedFile:
    matches = regex.search(filename)

    if matches is None:
        raise Exception("no match")

    song_title = matches["title"]

    day = matches["day"]
    month = matches["month"]
    year = matches["year"]

    if any(v is None for v in [day, month, year]):
        return ParsedFile(song_title, None, filename)
    else:
        return ParsedFile(
            song_title, date(int(f"20{year}"), int(month), int(day)), filename
        )


def parse_filename_v1(filename: str) -> ParsedFile: ...
