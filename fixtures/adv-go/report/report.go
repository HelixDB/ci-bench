// Package report summarises JSON lists. It imports no other package of the module.
package report

import (
	"encoding/json"
	"strings"
)

// Summary is the list's items, joined with "+".
func Summary(data []byte) (string, error) {
	var items []string
	if err := json.Unmarshal(data, &items); err != nil {
		return "", err
	}
	return strings.Join(items, "+"), nil
}
