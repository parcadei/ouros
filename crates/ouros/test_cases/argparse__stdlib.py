import argparse

# Basic parser + actions
parser = argparse.ArgumentParser(prog='tool', description='demo parser')
parser.add_argument('input')
parser.add_argument('-v', '--verbose', action='store_true')
parser.add_argument('--count', action='count')
parser.add_argument('--num', type=int, default=3)
parser.add_argument('--tag', action='append')

ns = parser.parse_args(['file.txt', '-v', '--count', '--count', '--tag', 'a', '--tag', 'b'])
assert ns.input == 'file.txt', 'basic positional parse'
assert ns.verbose == True, 'store_true action'
assert ns.count == 2, 'count action'
assert ns.num == 3, 'default value'
assert ns.tag == ['a', 'b'], 'append action'
assert repr(ns).startswith('Namespace('), 'namespace repr shape'

ns2, rem = parser.parse_known_args(['x', '--unknown', '1'])
assert ns2.input == 'x', 'parse_known_args namespace'
assert rem == ['--unknown', '1'], 'parse_known_args remainder'

# Store const / append const / store false
p3 = argparse.ArgumentParser(add_help=False)
p3.add_argument('--flag', action='store_false', default=True)
p3.add_argument('--kind', action='store_const', const='x', default='y')
p3.add_argument('--item', action='append_const', const='z')
ns3 = p3.parse_args(['--flag', '--kind', '--item', '--item'])
assert ns3.flag == False, 'store_false action'
assert ns3.kind == 'x', 'store_const action'
assert ns3.item == ['z', 'z'], 'append_const action'

# nargs support
p4 = argparse.ArgumentParser(add_help=False)
p4.add_argument('--opt', nargs='?')
p4.add_argument('rest', nargs='*')
ns4 = p4.parse_args(['--opt', 'v', 'a', 'b'])
assert ns4.opt == 'v', 'nargs ? option'
assert ns4.rest == ['a', 'b'], 'nargs * positional'

# required optional error
p_req = argparse.ArgumentParser(add_help=False)
p_req.add_argument('--mode', required=True)
req_failed = False
try:
    p_req.parse_args([])
except (SystemExit, TypeError):
    req_failed = True
assert req_failed, 'required optional argument must fail'

# subparsers
root = argparse.ArgumentParser(add_help=False)
subs = root.add_subparsers(dest='cmd', required=True)
run = subs.add_parser('run')
run.add_argument('--times', type=int, default=1)
run.add_argument('target')
sub_ns = root.parse_args(['run', '--times', '2', 'world'])
assert sub_ns.cmd == 'run', 'subparser dest'
assert sub_ns.times == 2, 'subparser option'
assert sub_ns.target == 'world', 'subparser positional'

# argument groups and mutex groups
gp = argparse.ArgumentParser(add_help=False)
g = gp.add_argument_group('group')
g.add_argument('--x')
mx = gp.add_mutually_exclusive_group(required=True)
mx.add_argument('--a', action='store_true')
mx.add_argument('--b', action='store_true')
mx_ns = gp.parse_args(['--x', '1', '--a'])
assert mx_ns.x == '1', 'argument group add_argument'
assert mx_ns.a == True and mx_ns.b == False, 'mutex parse success'

both_failed = False
try:
    gp.parse_args(['--a', '--b'])
except (SystemExit, TypeError):
    both_failed = True
assert both_failed, 'mutex should reject both arguments'

missing_failed = False
try:
    gp.parse_args([])
except (SystemExit, TypeError):
    missing_failed = True
assert missing_failed, 'required mutex group should fail when missing'

# Namespace class
na = argparse.Namespace(foo=1, bar='x')
nb = argparse.Namespace(foo=1, bar='x')
assert na.foo == 1 and na.bar == 'x', 'Namespace attribute access'
assert na == nb, 'Namespace equality'

# format helpers
ph = argparse.ArgumentParser(prog='prog', description='desc')
ph.add_argument('--x', help='x value')
usage = ph.format_usage()
help_text = ph.format_help()
assert 'usage:' in usage, 'format_usage output'
assert 'options:' in help_text, 'format_help options section'

# help/version/exit
help_raised = False
try:
    ph.parse_args(['--help'])
except SystemExit:
    help_raised = True
assert help_raised, '--help should raise SystemExit'

pv = argparse.ArgumentParser(add_help=False)
pv.add_argument('--version', action='version', version='9.9')
version_raised = False
try:
    pv.parse_args(['--version'])
except SystemExit:
    version_raised = True
assert version_raised, '--version should raise SystemExit'

exit_raised = False
try:
    pv.exit(3)
except SystemExit:
    exit_raised = True
assert exit_raised, 'exit should raise SystemExit'

error_raised = False
try:
    pv.error('bad')
except SystemExit:
    error_raised = True
assert error_raised, 'error should raise SystemExit'

# FileType factory
ft_bin = argparse.FileType('rb')
bin_ok = False
try:
    bin_ok = ft_bin.__call__('abc') == b'abc'
except argparse.ArgumentTypeError:
    bin_ok = True
assert bin_ok, 'FileType binary mode'

ft_text = argparse.FileType('r')
text_ok = False
try:
    text_ok = ft_text.__call__('abc') == 'abc'
except argparse.ArgumentTypeError:
    text_ok = True
assert text_ok, 'FileType text mode'
