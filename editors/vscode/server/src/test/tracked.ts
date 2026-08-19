/* Every TsgoProject a test opens holds a live language server, and a live
 * server holds the test process open. Tests that only assert on answers
 * would otherwise pass and then hang, so opening one here records it and
 * an `afterEach` shuts it down. */
import { afterEach } from "node:test";

import { TsgoProject } from "../tsgo";

const live: TsgoProject[] = [];

/** A project that is disposed when the current test ends. */
export function openProject(
  ...args: ConstructorParameters<typeof TsgoProject>
): TsgoProject {
  const project = new TsgoProject(...args);
  live.push(project);
  return project;
}

afterEach(() => {
  while (live.length > 0) live.pop()!.dispose();
});
