from typing_extensions import assert_type

from app import parse_count


bad_count: str = parse_count("42")
assert_type(parse_count("42"), str)
