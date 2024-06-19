from dataclasses import dataclass
from datetime import date
import re
from typing import Optional


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