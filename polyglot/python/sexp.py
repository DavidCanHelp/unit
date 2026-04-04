"""Minimal S-expression parser for the unit mesh protocol."""


def parse(s):
    """Parse an S-expression string into nested Python structures."""
    s = s.strip()
    if not s:
        raise ValueError("empty input")
    result, _ = _parse_expr(s, 0)
    return result


def _parse_expr(s, pos):
    pos = _skip_ws(s, pos)
    if pos >= len(s):
        raise ValueError("unexpected end of input")
    if s[pos] == '(':
        return _parse_list(s, pos)
    elif s[pos] == '"':
        return _parse_string(s, pos)
    else:
        return _parse_atom(s, pos)


def _parse_list(s, pos):
    pos += 1  # skip '('
    items = []
    while True:
        pos = _skip_ws(s, pos)
        if pos >= len(s):
            raise ValueError("unterminated list")
        if s[pos] == ')':
            return items, pos + 1
        item, pos = _parse_expr(s, pos)
        items.append(item)


def _parse_string(s, pos):
    pos += 1  # skip opening quote
    chars = []
    while pos < len(s):
        if s[pos] == '\\' and pos + 1 < len(s):
            chars.append(s[pos + 1])
            pos += 2
            continue
        if s[pos] == '"':
            return ''.join(chars), pos + 1
        chars.append(s[pos])
        pos += 1
    raise ValueError("unterminated string")


def _parse_atom(s, pos):
    start = pos
    while pos < len(s) and s[pos] not in ' \t\n\r()\"':
        pos += 1
    token = s[start:pos]
    try:
        return int(token), pos
    except ValueError:
        return token, pos


def _skip_ws(s, pos):
    while pos < len(s) and s[pos] in ' \t\n\r':
        pos += 1
    return pos


def get_keyword(parsed, name):
    """Extract the value following :name in a parsed list."""
    if not isinstance(parsed, list):
        return None
    key = ':' + name
    for i, item in enumerate(parsed):
        if item == key and i + 1 < len(parsed):
            return parsed[i + 1]
    return None


def msg_type(parsed):
    """Return the first atom of a parsed list (message type)."""
    if isinstance(parsed, list) and len(parsed) > 0 and isinstance(parsed[0], str):
        return parsed[0]
    return None


def format_sexp(data):
    """Serialize Python data back to an S-expression string."""
    if isinstance(data, list):
        return '(' + ' '.join(format_sexp(x) for x in data) + ')'
    elif isinstance(data, str):
        if data.startswith(':') or data.replace('-', '').isalpha():
            return data
        return '"' + data.replace('"', '\\"') + '"'
    elif isinstance(data, int):
        return str(data)
    return str(data)
