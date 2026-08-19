#!/usr/bin/env node

import path from "node:path";
import { pathToFileURL } from "node:url";

const tsgoRoot = process.env.RLC_TSGO_ROOT ?? "/Users/seominyeong/orca/typescript-go";
const fromTsgo = (relative) => pathToFileURL(path.join(tsgoRoot, relative)).href;

const [{ API, EmitOnly }, { createVirtualFileSystem }] = await Promise.all([
  import(fromTsgo("_packages/native-preview/src/api/sync/api.ts")),
  import(fromTsgo("_packages/native-preview/src/api/fs.ts")),
]);

const stateTs = `import type { State } from "./user";

export function afterIdle(state: State) {
  if (state !== "idle") {
    const narrowed = state;
    return narrowed;
  }
  return state;
}

class Store {
  set(key: string, value: number) {
    return value;
  }
}

export function mutations() {
  const map = new Map<string, number>();
  map.set("x", 1);

  const store = new Store();
  store.set("x", 1);
}
`;

const files = {
  "/tsconfig.json": JSON.stringify({
    compilerOptions: {
      strict: true,
      declaration: true,
      module: "nodenext",
      moduleResolution: "nodenext",
      target: "es2022",
      lib: ["es2022"],
    },
    files: ["/src/user.ts", "/src/state.ts"],
  }),
  "/src/user.ts": `export type State = "idle" | "loading" | "done";\n`,
  "/src/state.ts": stateTs,
};

const api = new API({
  cwd: "/",
  fs: createVirtualFileSystem(files),
});

try {
  const snapshot = api.updateSnapshot({ openProject: "/tsconfig.json" });
  const project = snapshot.getProject("/tsconfig.json");
  if (!project) throw new Error("tsgo did not open /tsconfig.json");

  const diagnostics = [
    ...project.program.getConfigFileParsingDiagnostics(),
    ...project.program.getSyntacticDiagnostics(),
    ...project.program.getSemanticDiagnostics(),
  ];
  if (diagnostics.length) {
    throw new Error(`unexpected diagnostics: ${JSON.stringify(diagnostics, null, 2)}`);
  }

  const narrowedPos = stateTs.indexOf("state;", stateTs.indexOf("const narrowed"));
  const narrowedType = project.checker.getTypeAtPosition("/src/state.ts", narrowedPos);
  const narrowedText = narrowedType ? project.checker.typeToString(narrowedType) : "<none>";

  const mapSetPos = stateTs.indexOf("set", stateTs.indexOf("map.set"));
  const storeSetPos = stateTs.indexOf("set", stateTs.indexOf("store.set"));
  const mapSet = project.checker.getSymbolAtPosition("/src/state.ts", mapSetPos);
  const storeSet = project.checker.getSymbolAtPosition("/src/state.ts", storeSetPos);

  const dts = project.program.emitToString(EmitOnly.OnlyDts);
  const dtsFiles = [...dts.outputFiles.keys()].sort();

  const result = {
    tsgoRoot,
    projects: snapshot.getProjects().map((p) => p.configFileName),
    roots: project.rootFiles,
    narrowedType: narrowedText,
    mapSet: {
      name: mapSet?.name,
      declarationPaths: mapSet?.declarations.map((d) => d.path) ?? [],
    },
    storeSet: {
      name: storeSet?.name,
      declarationPaths: storeSet?.declarations.map((d) => d.path) ?? [],
    },
    declarationEmit: {
      emitSkipped: dts.emitSkipped,
      files: dtsFiles,
    },
  };

  console.log(JSON.stringify(result, null, 2));

  const narrowedParts = new Set(narrowedText.split(" | "));
  if (
    narrowedParts.size !== 2 ||
    !narrowedParts.has('"loading"') ||
    !narrowedParts.has('"done"')
  ) {
    throw new Error(`expected narrowed type '"loading" | "done", got ${narrowedText}`);
  }
  if (!result.mapSet.declarationPaths.some((p) => p.includes("lib.es2015.collection.d.ts"))) {
    throw new Error("expected Map#set to resolve to the TypeScript lib declaration");
  }
  if (!result.storeSet.declarationPaths.includes("/src/state.ts")) {
    throw new Error("expected Store#set to resolve to the user source file");
  }
  if (!dtsFiles.includes("/src/state.d.ts")) {
    throw new Error("expected declaration emit for /src/state.ts");
  }
} finally {
  api.close();
}
