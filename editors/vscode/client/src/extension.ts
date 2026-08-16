/* --------------------------------------------------------------------------
 * rl language client — follows the official VS Code LSP extension pattern
 * (client launches the server over Node IPC and wires the `rl` language).
 * ----------------------------------------------------------------------- */
import * as path from "path";
import { ExtensionContext, workspace } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: ExtensionContext): void {
  const serverModule = context.asAbsolutePath(
    path.join("server", "out", "server.js"),
  );

  const serverOptions: ServerOptions = {
    run: { module: serverModule, transport: TransportKind.ipc },
    debug: {
      module: serverModule,
      transport: TransportKind.ipc,
      options: { execArgv: ["--nolazy", "--inspect=6009"] },
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "rl" },
      { scheme: "untitled", language: "rl" },
    ],
    synchronize: {
      // Re-validate when the locally built compiler appears or changes.
      fileEvents: workspace.createFileSystemWatcher(
        "**/target/{debug,release}/rlc",
      ),
    },
  };

  client = new LanguageClient(
    "rl",
    "rl Language Server",
    serverOptions,
    clientOptions,
  );
  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
