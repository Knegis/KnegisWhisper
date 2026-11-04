/** Parses the side-effect-free command-line contracts for visual-check tools. */
const SHELL_USAGE = `Usage: node tools/visual-check/shell-title-bar.mjs [options]

  --url <url>                 Dev server URL (default: http://127.0.0.1:5183/)
  --out <directory>           Report directory (default: timestamped out directory)
  --edge <path>               Path to msedge.exe
  --port <number>             Edge debugging port (default: 9223)
  --keep-fixture-banner       Do not remove the invented-data banner
  --help                      Print this usage`;

const APP_USAGE = `Usage: node tools/visual-check/app-windows.mjs <command> [options]

Commands: status, start, watch, theme, stop, run-all

  --port <number>             WebView2 debugging port (default: 9222)
  --out <directory>           Report directory (default: timestamped out directory)
  --provider-label <text>     Provider text to select (default: local streaming)
  --system-audio <boolean>    Capture participant audio (default: true)
  --via <ui|invoke>           Drive controls or invoke directly (default: ui)
  --timeout <seconds>         Recording/stop timeout (default: 180)
  --seconds <seconds>         Watch duration (default: 60)
  --help                      Print this usage`;

function timestampedOut() {
  const stamp = new Date()
    .toISOString()
    .replace(/[-:]/g, "")
    .replace(/\.\d{3}Z$/, "")
    .replace("T", "-");
  return `tools/visual-check/out/${stamp}`;
}

function number(value, flag) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) {
    throw new Error(`${flag} requires a non-negative number`);
  }
  return parsed;
}

function boolean(value, flag) {
  if (value === "true") return true;
  if (value === "false") return false;
  throw new Error(`${flag} requires true or false`);
}

function take(argv, index, flag) {
  const value = argv[index + 1];
  if (value === undefined || value.startsWith("--")) {
    throw new Error(`${flag} requires a value`);
  }
  return value;
}

export function shellUsage() {
  return SHELL_USAGE;
}

export function appUsage() {
  return APP_USAGE;
}

export function parseShellArgs(argv) {
  const result = {
    url: "http://127.0.0.1:5183/",
    out: timestampedOut(),
    edge: undefined,
    port: 9223,
    keepFixtureBanner: false,
    help: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    if (flag === "--help") result.help = true;
    else if (flag === "--keep-fixture-banner") result.keepFixtureBanner = true;
    else if (flag === "--url") {
      result.url = take(argv, i, flag);
      i += 1;
    } else if (flag === "--out") {
      result.out = take(argv, i, flag);
      i += 1;
    } else if (flag === "--edge") {
      result.edge = take(argv, i, flag);
      i += 1;
    } else if (flag === "--port") {
      result.port = number(take(argv, i, flag), flag);
      i += 1;
    } else {
      throw new Error(`Unknown option: ${flag}`);
    }
  }
  return result;
}

export function parseAppArgs(argv) {
  const result = {
    command: undefined,
    port: 9222,
    out: timestampedOut(),
    providerLabel: "local streaming",
    systemAudio: true,
    via: "ui",
    timeout: 180,
    seconds: 60,
    help: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    if (flag === "--help") result.help = true;
    else if (!flag.startsWith("--") && result.command === undefined) {
      result.command = flag;
    } else if (flag === "--port") {
      result.port = number(take(argv, i, flag), flag);
      i += 1;
    } else if (flag === "--out") {
      result.out = take(argv, i, flag);
      i += 1;
    } else if (flag === "--provider-label") {
      result.providerLabel = take(argv, i, flag);
      i += 1;
    } else if (flag === "--system-audio") {
      result.systemAudio = boolean(take(argv, i, flag), flag);
      i += 1;
    } else if (flag === "--via") {
      result.via = take(argv, i, flag);
      i += 1;
      if (!["ui", "invoke"].includes(result.via)) {
        throw new Error("--via must be ui or invoke");
      }
    } else if (flag === "--timeout") {
      result.timeout = number(take(argv, i, flag), flag);
      i += 1;
    } else if (flag === "--seconds") {
      result.seconds = number(take(argv, i, flag), flag);
      i += 1;
    } else {
      throw new Error(`Unknown option: ${flag}`);
    }
  }
  if (
    !result.help
    && !["status", "start", "watch", "theme", "stop", "run-all"].includes(
      result.command,
    )
  ) {
    throw new Error("Expected a command: status, start, watch, theme, stop, or run-all");
  }
  return result;
}
