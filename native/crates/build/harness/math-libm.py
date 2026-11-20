"""python3 -I math-libm.py <math-probe dir>

Whether V8's Math.pow on this machine is the C library's pow: computes
pow(|x|, y) for the probe's inputs with this machine's libm (Python's
math.pow calls C pow), and x ** 1.5 for x >= 0, and counts the results that
differ from the probe's (pow.txt, the '#starstar.txt' section of fns.txt).
Also prints the NaN bit patterns the probe holds for sqrt of a negative
input, and whether pow-cold.txt (the interpreter) equals the first 2,000
lines of pow.txt (the optimised loop)."""

import math
import struct
import sys

d = sys.argv[1]
inputs = [struct.unpack('>d', bytes.fromhex(h))[0] for h in open(f'{d}/inputs.txt').read().split('\n')]
pow_lines = open(f'{d}/pow.txt').read().split('\n')
cold = open(f'{d}/pow-cold.txt').read().split('\n')
sections, name = {}, None
for line in open(f'{d}/fns.txt').read().split('\n'):
    if line.startswith('#'):
        name = line[1:].replace('.txt', '')
        sections[name] = []
    else:
        sections[name].append(line)
bits = lambda v: struct.pack('>d', v).hex()
ys = [0.5, 2, 3, -1.5, 1 / 3, 10, 0.1]
differ = 0
for i, x in enumerate(inputs):
    for j, y in enumerate(ys):
        try:
            v = math.pow(abs(x), y)
        except (OverflowError, ValueError, ZeroDivisionError):
            v = math.inf if abs(x) == 0 and y < 0 else math.nan
        if bits(v) != pow_lines[i * 7 + j]:
            differ += 1
print(f'pow: {differ} of {len(pow_lines)} differ from this libm')
differ = sum(1 for i, x in enumerate(inputs) if x >= 0 and bits(math.pow(x, 1.5)) != sections['starstar'][i])
print(f'x ** 1.5 for x >= 0: {differ} of {sum(1 for x in inputs if x >= 0)} differ from this libm')
nans = sorted({sections['sqrt'][i] for i, x in enumerate(inputs) if x < 0})
print(f'sqrt of a negative input: bit patterns {nans}')
nans = sorted({sections['starstar'][i] for i, x in enumerate(inputs) if x < 0})
print(f'x ** 1.5 of a negative input: bit patterns {nans}')
print(f'pow interpreted (pow-cold) equals the optimised loop: {cold == pow_lines[:len(cold)]}')
