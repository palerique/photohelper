#!/usr/bin/env bash
# D5c E2E: verify photohelper-test-helpers is used only as a [dev-dependency].
# Any non-dev consumer would link the test helpers into a release artifact —
# a policy violation per crates/photohelper-test-helpers/Cargo.toml.
set -euo pipefail

# --no-deps suppresses the resolve section; omit it to get dep_kinds.
cargo metadata --format-version 1 | python3 -c "
import sys, json

data = json.load(sys.stdin)
nodes = (data.get('resolve') or {}).get('nodes', [])
violations = []
for node in nodes:
    for dep in node.get('deps', []):
        if dep['name'] == 'photohelper_test_helpers':
            for dk in dep.get('dep_kinds', []):
                # kind=None means regular dep; kind='dev' is the only acceptable kind.
                if dk.get('kind') != 'dev':
                    violations.append((node['id'].split(' ')[0], str(dk.get('kind'))))

if violations:
    print('FAIL: photohelper-test-helpers has non-dev consumers:', violations, file=sys.stderr)
    sys.exit(1)
else:
    print('PASS: photohelper-test-helpers is dev-only')
"
