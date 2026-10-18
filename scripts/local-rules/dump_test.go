// This file is not compiled by promptguard-cli. scripts/local-rules/regenerate.sh
// copies it into a scratch copy of the source rule set's
// internal/security/detection package and runs it there, so it can read the detectors' unexported pattern tables.
// It writes the pattern pack (rules plus the scoring pipeline they run under)
// and the corpus that tests/local_rules_tests.rs checks the Rust port against.

package detection

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"SOURCE_MODULE/internal/security"
)

type packPattern struct {
	ID          string  `json:"id"`
	Detector    string  `json:"detector"`
	Category    string  `json:"category"`
	Severity    string  `json:"severity"`
	Confidence  float64 `json:"confidence"`
	MinEntropy  float64 `json:"min_entropy,omitempty"`
	Description string  `json:"description,omitempty"`
	Regex       string  `json:"regex"`
}

// The detectors this pack carries, and the ones it leaves out with the
// reason. The CLI prints none of this; it is here so the next person to
// regenerate the pack sees why each choice was made.
var portedFiles = []string{"injection.go", "jailbreak.go", "indirect_injection.go", "secrets.go", "../steps.go", "../engine.go"}

// Rules taken out of the pack entirely. Only noise in a repository scan:
// the rule's own effect can never exceed warn under the source's thresholds.
var dropped = map[string]string{
	"jailbreak/authority.admin_override#4": "slash_admin matches any /dev, /root, /admin or /debug path (/dev/null in every shell script, /admin routes in every web app); under the source's jailbreak thresholds it can only warn, so in a file scan it is pure noise",
}

// Rules kept but capped at warn: under the source's thresholds they block, and in
// a file scan they block ordinary system prompts and documentation.
var demoted = map[string]string{
	"injection/override.imperative": "blocks at exactly the 0.72 threshold on any imperative near \"the ... rules/context/message\" within one sentence (\"skip the rules section\", \"override the default message\"); the narrow override.* rules still block the real phrasings",
	"injection/extraction.direct":   "blocks \"Never reveal your system prompt\" and \"Summarize the context below\", the defensive and task lines most system prompts are made of; it fires on the noun, not on intent",
}

var excluded = map[string]string{
	"pii.go":                          "PII is redaction, a separate API operation from a threat scan; several rules also need code validators (Luhn, checksums) that are not regexes",
	"exfil.go":                        "response-side: it flags model output, and rules such as canonical_sysprompt fire on ordinary system prompts a CI scan would be pointed at",
	"structural.go":                   "not a pattern list: fake-turn counting and encoding-density heuristics are code",
	"crescendo.go":                    "multi-turn: needs conversation history a single scan does not have",
	"topic.go":                        "per-tenant topic policy, configured at run time, with no default rules",
	"jailbreak.go structuralFindings": "the jailbreak detector's non-regex layer (many-shot turn counting, encoding density) is code, not patterns",
}

type provenance struct {
	Source         string            `json:"source"`
	License        string            `json:"license"`
	Files          []sourceFile      `json:"files"`
	GeneratedOn    string            `json:"generated_on"`
	RegenerateWith string            `json:"regenerate_with"`
	MatchSemantics []string          `json:"match_semantics"`
	Excluded       map[string]string `json:"excluded"`
	Dropped        map[string]string `json:"dropped"`
	Demoted        map[string]string `json:"demoted"`
}

// The scoring pipeline, read from the source rather than restated: a finding's
// weighted score is severity_scores[severity] x confidence, and each input
// step applies its strategy when a detector's best weighted score reaches its
// threshold (DefaultInputSteps, steps.go).
type pipeline struct {
	SeverityScores map[string]float64 `json:"severity_scores"`
	InputSteps     []pipelineStep     `json:"input_steps"`
}

type pipelineStep struct {
	Detector  string  `json:"detector"`
	Strategy  string  `json:"strategy"`
	Threshold float64 `json:"threshold"`
}

// A list rather than a name-to-hash map: a "secrets.go": "<hash>" pair is
// exactly what the push-time secret scanner's generic rule looks for.
type sourceFile struct {
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
}

type packFile struct {
	Provenance provenance    `json:"provenance"`
	Pipeline   pipeline      `json:"pipeline"`
	Patterns   []packPattern `json:"patterns"`
}

// Matches is what each Go regex matched (rule by rule). The two verdicts are
// what the source's own engine decided, running DefaultInputSteps over the sample
// as user content and as tool output.
type corpusSample struct {
	Name             string   `json:"name"`
	Text             string   `json:"text"`
	Benign           bool     `json:"benign"`
	Matches          []string `json:"source_matches"`
	VerdictUser      string   `json:"source_verdict_user"`
	VerdictUntrusted string   `json:"source_verdict_untrusted"`
	// Structural is true when the jailbreak detector's non-regex layer
	// (structuralFindings, not ported) produced findings on the sample.
	Structural bool `json:"source_structural"`
}

func TestDumpOfflinePack(t *testing.T) {
	out := os.Getenv("PACK_OUT_DIR")
	if out == "" {
		t.Skip("PACK_OUT_DIR not set")
	}

	var pack []packPattern
	type compiled struct {
		p     packPattern
		match func(string) bool
	}
	var all []compiled
	seen := map[string]int{}
	add := func(p packPattern, match func(string) bool) {
		base := p.Detector + "/" + p.ID
		seen[base]++
		p.ID = base
		if n := seen[base]; n > 1 {
			p.ID = fmt.Sprintf("%s#%d", base, n)
		}
		// A dropped rule still records its matches in the corpus, so the
		// tests can tell which samples it would have changed.
		all = append(all, compiled{p, match})
		if _, drop := dropped[p.ID]; drop {
			return
		}
		pack = append(pack, p)
	}

	for _, p := range NewInjectionDetector().patterns {
		re := p.regex
		add(packPattern{p.subCategory, "injection", p.category, string(p.severity), p.confidence, 0, "", re.String()},
			func(s string) bool { return re.MatchString(strings.ToLower(s)) })
	}
	for _, p := range NewJailbreakDetector().patterns {
		re := p.regex
		add(packPattern{p.subCategory, "jailbreak", p.category, string(p.severity), p.confidence, 0, p.description, re.String()},
			re.MatchString)
	}
	for _, p := range NewIndirectInjectionDetector().patterns {
		re := p.regex
		add(packPattern{p.category, "indirect_injection", p.category, string(p.severity), p.confidence, 0, "", re.String()},
			re.MatchString)
	}
	for _, p := range NewSecretsDetector().prefixes {
		re, minEntropy := p.regex, p.minEntropy
		add(packPattern{p.kind, "secrets", p.kind, string(p.severity), p.confidence, minEntropy, "", re.String()},
			func(s string) bool {
				for _, m := range re.FindAllString(s, -1) {
					if minEntropy == 0 || shannonEntropy(m) >= minEntropy {
						return true
					}
				}
				return false
			})
	}

	// Parity corpus: the source's handwritten eval sets plus the jailbreak
	// corpus. secrets.json and pii.json are left out on purpose — their
	// samples are credential-shaped and would trip the secret scanner on
	// push; the Rust tests build secret-shaped strings at run time instead.
	var samples []corpusSample
	for _, f := range []string{"injection.json", "jailbreak.json", "benign.json"} {
		raw, err := os.ReadFile(filepath.Join("testdata", "eval", f))
		if err != nil {
			t.Fatal(err)
		}
		var es []evalSample
		if err := json.Unmarshal(raw, &es); err != nil {
			t.Fatal(err)
		}
		for _, e := range es {
			samples = append(samples, corpusSample{Name: "eval/" + e.Name, Text: e.Text, Benign: len(e.Labels) == 0})
		}
	}
	for _, c := range positiveCorpus {
		samples = append(samples, corpusSample{Name: "corpus/" + c.name, Text: c.prompt})
	}
	for _, c := range negativeCorpus {
		samples = append(samples, corpusSample{Name: "corpus/" + c.name, Text: c.prompt, Benign: true})
	}
	ported := map[string]bool{}
	for _, f := range []string{"injection", "jailbreak", "indirect_injection", "secrets"} {
		ported[f] = true
	}
	var steps []security.Step
	var pipeSteps []pipelineStep
	for _, st := range security.DefaultInputSteps() {
		if ported[st.Detector] {
			steps = append(steps, st)
			pipeSteps = append(pipeSteps, pipelineStep{st.Detector, string(st.Strategy), st.Threshold})
		}
	}
	engine := security.NewEngine(NewInjectionDetector(), NewJailbreakDetector(), NewIndirectInjectionDetector(), NewSecretsDetector())
	verdict := func(text, role string) string {
		return string(engine.RunSteps(context.Background(), text, steps, &security.RunOptions{Role: role}).Action)
	}
	for i := range samples {
		samples[i].VerdictUser = verdict(samples[i].Text, security.RoleUser)
		samples[i].VerdictUntrusted = verdict(samples[i].Text, security.RoleTool)
		samples[i].Structural = len(structuralFindings(samples[i].Text)) > 0
		samples[i].Matches = []string{}
		for _, c := range all {
			if c.match(samples[i].Text) {
				samples[i].Matches = append(samples[i].Matches, c.p.ID)
			}
		}
	}

	write := func(name string, v any) {
		f, err := os.Create(filepath.Join(out, name))
		if err != nil {
			t.Fatal(err)
		}
		defer f.Close()
		enc := json.NewEncoder(f)
		enc.SetEscapeHTML(false)
		enc.SetIndent("", "  ")
		if err := enc.Encode(v); err != nil {
			t.Fatal(err)
		}
	}
	var files []sourceFile
	for _, f := range portedFiles {
		raw, err := os.ReadFile(f)
		if err != nil {
			t.Fatal(err)
		}
		sum := sha256.Sum256(raw)
		files = append(files, sourceFile{f, hex.EncodeToString(sum[:])})
	}
	write("pack.json", packFile{
		Provenance: provenance{
			Source:         "the copyright holder's own earlier private rule set, internal/security/detection (Go regexp, RE2 syntax)",
			License:        "The patterns and corpus were ported by the copyright holder from their own earlier private rule set, and are licensed by that copyright holder under this repository's Apache-2.0 licence.",
			Files:          files,
			GeneratedOn:    os.Getenv("GENERATED_ON"),
			RegenerateWith: "scripts/local-rules/regenerate.sh <path-to-source-checkout>",
			MatchSemantics: []string{
				"injection rules match case-insensitively; the source lowercases the input before matching, which is equivalent except that it left the uppercase [INST] rules unable to match anything",
				"rules with min_entropy count a match only when its Shannon entropy (bits per character) is at least that value",
				"findings are scored and gated by the pipeline block below, as the source's DefaultInputSteps do, including each detector's combination bonus",
				"indirect_injection rules run only on content marked untrusted (the source runs them only for tool, retrieval and memory roles)",
				"a rule in demoted can at most warn; a rule in dropped is not in the pack",
			},
			Excluded: excluded,
			Dropped:  dropped,
			Demoted:  demoted,
		},
		Pipeline: pipeline{
			SeverityScores: map[string]float64{
				"critical": severityToScore(security.SeverityCritical),
				"high":     severityToScore(security.SeverityHigh),
				"medium":   severityToScore(security.SeverityMedium),
			},
			InputSteps: pipeSteps,
		},
		Patterns: pack,
	})
	write("corpus.json", samples)
}
