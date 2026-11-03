// The Durable Streams Node reference server (@durable-streams/server, pinned in
// package.json), started the way the RFC-0004 Spike B tests need it.
//
//   node server.mjs --port 4437 [--data-dir DIR] [--host 127.0.0.1] [--long-poll-ms 30000]
//
// Without --data-dir storage is in memory and a restart loses every stream.
// With --data-dir it is the server's file-backed store (log files + LMDB
// metadata), which survives a restart.
//
// The reference server has no authentication and no authorisation (see
// docs/rfc/0004-spike-b-results.md). Do not expose it.
import { DurableStreamTestServer } from "@durable-streams/server"

const args = process.argv.slice(2)
const opt = (name, dflt) => {
  const i = args.indexOf(`--${name}`)
  return i >= 0 ? args[i + 1] : dflt
}

const port = Number(opt("port", "4437"))
const host = opt("host", "127.0.0.1")
const dataDir = opt("data-dir", undefined)
const longPollTimeout = Number(opt("long-poll-ms", "30000"))

const server = new DurableStreamTestServer({ port, host, dataDir, longPollTimeout })
const url = await server.start()
console.log(`READY ${url} storage=${dataDir ? `file:${dataDir}` : "memory"}`)

const stop = async (sig) => {
  console.log(`STOPPING on ${sig}`)
  // server.stop() waits for keep-alive connections; do not wait for ever.
  setTimeout(() => process.exit(0), 3000).unref()
  try {
    await server.stop()
  } finally {
    process.exit(0)
  }
}
process.on("SIGTERM", () => stop("SIGTERM"))
process.on("SIGINT", () => stop("SIGINT"))
