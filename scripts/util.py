from dataclasses import dataclass
from datetime import date
import re


v3_date_regex = re.compile(
    r"(?P<title>.+) \((?P<full_date>(?P<day>\d?\d) (?P<month>\d?\d) (?P<year>\d\d))\)"
)
v1_date_regex = re.compile(
    r"\[(?P<full_date>(?P<month>\d?\d)(?:-|／)(?P<day>\d?\d)(?:-|／)(?P<year>\d\d))\] (?P<title>.+)(?: \[\d+\])?\..+"
)
evil_date_regex = re.compile(
    r"(?P<title>.+) \((?P<full_date>(?P<day>\d?\d)(?: \(evil\))? (?P<month>\d?\d) (?P<year>\d\d))\)(?: \(evil\))?\..+"
)
duet_date_regex = v3_date_regex


@dataclass
class ParsedFile:
    title: str
    date: date
    filename: str


def parse_filename(filename: str) -> ParsedFile:
    regexes = [v3_date_regex, v1_date_regex, evil_date_regex, duet_date_regex]

    for regex in regexes:
        try:
            parsed = apply_regex(filename, regex)
            return parsed
        except Exception as _:
            continue

    raise Exception("no matches from any provider")


def apply_regex(filename: str, regex: re.Pattern[str]) -> ParsedFile:
    matches = v3_date_regex.search(filename)

    if matches is None:
        raise Exception("no match")

    song_title = matches["title"]

    day = matches["day"]
    month = matches["month"]
    year = matches["year"]

    return ParsedFile(
        song_title, date(int(f"20{year}"), int(month), int(day)), filename
    )


def parse_filename_v1(filename: str) -> ParsedFile: ...
