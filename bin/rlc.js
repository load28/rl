#!/usr/bin/env node
/**
 * rlc — compile .rl files to .ts.
 *
 *   rlc file.rl [more.rl ...]      writes file.ts next to each input
 *   rlc src/                       compiles every .rl under src/ recursively
 *   rlc -p file.rl                 prints the output to stdout
 *   rlc -o out/ src/               mirrors the input tree under out/
 *   rlc --check src/               compiles without writing anything
 */

import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { compile, RlError } from '../src/compile.js';

const VERSION = '0.1.0';

function usage() {
  console.log(`rlc v${VERSION} — rl to TypeScript compiler

Usage: rlc [options] <file.rl | dir> ...

Options:
  -o, --out-dir <dir>   write outputs under <dir> (mirrors input paths)
  -p, --print           print compiled output to stdout instead of writing
  --check               compile only; write nothing (syntax check)
  --no-banner           omit the "generated" banner comment
  -h, --help            show this help
  -v, --version         show version`);
}

function collectRlFiles(entry) {
  const stat = fs.statSync(entry);
  if (stat.isFile()) return [entry];
  if (stat.isDirectory()) {
    return fs
      .readdirSync(entry, { recursive: true })
      .map((p) => path.join(entry, p))
      .filter((p) => p.endsWith('.rl') && fs.statSync(p).isFile())
      .sort();
  }
  return [];
}

function main(argv) {
  const inputs = [];
  let outDir = null;
  let print = false;
  let check = false;
  let banner = true;

  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '-h' || a === '--help') { usage(); return 0; }
    if (a === '-v' || a === '--version') { console.log(VERSION); return 0; }
    if (a === '-p' || a === '--print') { print = true; continue; }
    if (a === '--check') { check = true; continue; }
    if (a === '--no-banner') { banner = false; continue; }
    if (a === '-o' || a === '--out-dir') {
      outDir = argv[++i];
      if (!outDir) { console.error('rlc: --out-dir requires a value'); return 1; }
      continue;
    }
    if (a.startsWith('-')) { console.error(`rlc: unknown option ${a}`); return 1; }
    inputs.push(a);
  }

  if (inputs.length === 0) {
    usage();
    return 1;
  }

  const jobs = [];
  for (const input of inputs) {
    if (!fs.existsSync(input)) {
      console.error(`rlc: no such file or directory: ${input}`);
      return 1;
    }
    const stat = fs.statSync(input);
    for (const file of collectRlFiles(input)) {
      let outPath;
      if (outDir) {
        const rel = stat.isDirectory() ? path.relative(input, file) : path.basename(file);
        outPath = path.join(outDir, rel.replace(/\.rl$/, '.ts'));
      } else {
        outPath = file.replace(/\.rl$/, '.ts');
      }
      jobs.push({ file, outPath });
    }
  }

  if (jobs.length === 0) {
    console.error('rlc: no .rl files found');
    return 1;
  }

  let failed = false;
  for (const { file, outPath } of jobs) {
    let code;
    try {
      const source = fs.readFileSync(file, 'utf8');
      code = compile(source, { filename: file }).code;
    } catch (err) {
      if (err instanceof RlError) {
        console.error(`rlc: ${err.message}`);
        failed = true;
        continue;
      }
      throw err;
    }
    if (banner) {
      code = `// @generated from ${path.basename(file)} by rlc — do not edit directly.\n${code}`;
    }
    if (print) {
      process.stdout.write(code);
    } else if (!check) {
      fs.mkdirSync(path.dirname(outPath), { recursive: true });
      fs.writeFileSync(outPath, code);
      console.error(`rlc: ${file} → ${outPath}`);
    }
  }
  return failed ? 1 : 0;
}

process.exit(main(process.argv.slice(2)));
