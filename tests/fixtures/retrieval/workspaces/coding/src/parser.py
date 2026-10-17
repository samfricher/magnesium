def parse_course_code(value):
    """Split a department prefix from a numeric course identifier."""
    return value.split("-", maxsplit=1)
