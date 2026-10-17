#!/usr/bin/env python3
"""Validate the frozen synthetic pack and emit only tutor-visible case inputs.

The operator passes tutor_prompts (not this entire manifest) to the existing
installed-host capture helper. This validates fixture identity, never teaching.
"""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PACK = ROOT / 'tests/fixtures/interaction/statistics'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(pack):
    fixture = json.loads((pack / 'cases.json').read_text())
    if fixture.get('synthetic') is not True or fixture.get('fixture_version') != 'statistics/v1':
        raise ValueError('expected frozen synthetic statistics/v1 pack')
    pairs, ids, cases, sources = {}, set(), [], {}
    for case in fixture['cases']:
        identity = case['id']
        if identity in ids:
            raise ValueError('duplicate case identity')
        ids.add(identity)
        for field in ['goal', 'learner_history', 'prompt']:
            if not isinstance(case[field], str) or not 0 < len(case[field]) <= 5000:
                raise ValueError(f'invalid {field}')
        if not isinstance(case.get('evaluator'), dict):
            raise ValueError('missing independent evaluation rubric')
        followups = case.get('followups', [])
        if not isinstance(followups, list) or any(not isinstance(x, str) or not 0 < len(x) <= 5000 for x in followups):
            raise ValueError('invalid followup')
        source = (pack / case['source']).resolve()
        if not source.is_relative_to(pack.resolve()) or not source.is_file():
            raise ValueError('source must be a file within the synthetic pack')
        sources[case['source']] = digest(source)
        if case.get('pair'):
            previous = pairs.setdefault(case['pair'], [])
            if previous and previous[0] != case['prompt']:
                raise ValueError('paired cases must retain the same surface prompt')
            previous.append(case['prompt'])
        # Deliberate projection: no evaluator dictionary, expected states,
        # acceptable actions, forbidden claims or pair labels enter prompts.
        first = f"I want to learn to {case['goal'][0].lower() + case['goal'][1:]}. Use the synthetic sources in this folder. {case['learner_history']} {case['prompt']}"
        cases.append({'id': identity, 'tutor_prompts': [first, *followups], 'source': case['source']})
    if any(len(group) != 2 for group in pairs.values()):
        raise ValueError('each paired comparison needs exactly two histories')
    sources['source-pages.pdf'] = digest(pack / 'source-pages.pdf')
    return {
        'fixture_version': fixture['fixture_version'],
        'fixture_sha256': digest(pack / 'cases.json'),
        'policy_sha256': digest(ROOT / 'src/workspace/instructions.md'),
        'source_sha256': sources,
        'cases': cases,
        'claim_limit': 'Input preflight only. Native behavior, live agent behavior and human learning require separate results.',
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, help='new private manifest file; never overwrite evidence')
    args = parser.parse_args()
    manifest = prepare(PACK)
    if args.output:
        import os
        fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, 'w') as stream:
            json.dump(manifest, stream, indent=2)
            stream.write('\n')
    print(f"Validated {len(manifest['cases'])} cases; fixture SHA-256 {manifest['fixture_sha256']}; evaluator annotations excluded.")


if __name__ == '__main__':
    main()
