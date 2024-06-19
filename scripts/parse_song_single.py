import json
import sys

from util import parse_filename

song_path = sys.argv[1]

try:
    parsed = parse_filename(song_path)
except Exception as e:
    print(json.dumps({"success": False, "error": repr(e)}))
    exit()

if parsed.date is None:
        print(
        json.dumps(
            {
                "success": True,
                "title": parsed.title,
                "date": None,
                "singer_id": 1,
            }
        )
    )
else:
    print(
        json.dumps(
            {
                "success": True,
                "title": parsed.title,
                "date": {
                    "day": parsed.date.day,
                    "month": parsed.date.month,
                    "year": parsed.date.year,
                },
                "singer_id": 1,
            }
        )
    )
    
