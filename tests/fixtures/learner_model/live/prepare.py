#!/usr/bin/env python3
"""Materialize separated live-role inputs; never starts a model or reads auth."""
import argparse
import json
from pathlib import Path

HERE=Path(__file__).resolve().parent
parser=argparse.ArgumentParser()
parser.add_argument('--case',required=True)
parser.add_argument('--conversation',type=int,default=1)
parser.add_argument('--out',type=Path,required=True)
args=parser.parse_args()
case=next(c for c in json.loads((HERE/'learner-cases.json').read_text())['cases'] if c['case_id']==args.case)
conversation=case['conversations'][args.conversation-1]
if args.out.exists():raise SystemExit('Use a new output directory; existing runs are never overwritten.')
for role in ['tutor','learner','evaluator']:(args.out/role).mkdir(parents=True)
(args.out/'tutor/system.txt').write_text((HERE/'tutor.txt').read_text())
(args.out/'tutor/opening.txt').write_text(conversation['opening_message']+'\n')
(args.out/'learner/system.txt').write_text((HERE/'learner.txt').read_text())
(args.out/'learner/setup.json').write_text(json.dumps({'allowed_resources':case['allowed_resources'],**conversation},indent=2)+'\n')
(args.out/'evaluator/system.txt').write_text((HERE/'evaluator.txt').read_text())
criteria=json.loads((HERE/'evaluator-cases.json').read_text())['criteria'][args.case]
(args.out/'evaluator/setup.json').write_text(json.dumps({'case':case,'criteria':criteria},indent=2)+'\n')
for resource in case['allowed_resources']:
    for role in ['tutor','learner']:
        (args.out/role/resource).write_text((HERE/resource).read_text())
(args.out/'run-status.json').write_text(json.dumps({'case_id':args.case,'conversation':args.conversation,'status':'NOT_RUN','qualification':'Inputs prepared only; no host/model invocation','limits':{'learner_messages':6,'tutor_tool_calls':24,'wall_minutes':12,'context_default_bytes':24576,'context_max_bytes':65536}},indent=2)+'\n')
print(args.out)
