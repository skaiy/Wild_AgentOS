package executor

import (
	"strings"
	"unicode"
)

func extractJSON(rawJSON string) string {
	raw := strings.TrimSpace(rawJSON)

	start := strings.Index(raw, "{")
	if start == -1 {
		return raw
	}

	depth := 0
	inString := false
	escaped := false
	end := -1

	for i, ch := range raw[start:] {
		if escaped {
			escaped = false
			continue
		}
		if ch == '\\' && inString {
			escaped = true
			continue
		}
		if ch == '"' {
			inString = !inString
			continue
		}
		if inString {
			continue
		}
		if ch == '{' {
			depth++
		} else if ch == '}' {
			depth--
			if depth == 0 {
				end = start + i + 1
				break
			}
		}
	}

	if end == -1 {
		return raw
	}

	return strings.TrimSpace(raw[start:end])
}

func trimLeadingWhitespace(s string) string {
	for i, r := range s {
		if !unicode.IsSpace(r) {
			return s[i:]
		}
	}
	return s
}