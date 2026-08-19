import { Result } from "@rl/std";

export enum Stage {
  Todo,
  Doing(owner: string),
  Done,
}

type Task = {
  id: number;
  title: string;
  stage: Stage;
  estimate: number;
};

function parseNumber(raw: string | undefined, field: string): Result<number, string> {
  const value = Number(raw);
  if (!Number.isFinite(value)) {
    return Result.Err(`bad ${field}: ${raw ?? "<missing>"}`);
  }
  return Result.Ok(value);
}

function parseStage(raw: string | undefined): Result<Stage, string> {
  return match (raw ?? "") {
    "todo" => Result.Ok(Stage.Todo),
    "doing" => Result.Ok(Stage.Doing("sam")),
    "done" => Result.Ok(Stage.Done),
    _ => Result.Err(`unknown stage: ${raw ?? "<missing>"}`),
  };
}

function parseTask(row: string): Result<Task, string> {
  return result {
    const parts = row.split("|");
    const id <- parseNumber(parts[0], "id");
    const stage <- parseStage(parts[2]);
    const estimate <- parseNumber(parts[3], "estimate");
    {
      id,
      title: parts[1] ?? "(untitled)",
      stage,
      estimate,
    }
  };
}

function stageLabel(stage: Stage): string {
  return match (stage) {
    Todo => "todo",
    Doing(owner) => `doing by ${owner}`,
    Done => "done",
  };
}

function describe(task: Task): string {
  return `${task.id}. ${task.title} (${stageLabel(task.stage)}, ${task.estimate}h)`;
}

function totalEstimate(val tasks: Task[]): number {
  return tasks
    .map((task) => task.estimate)
    |> .reduce((sum, hours) => sum + hours, 0);
}

const rows = [
  "1|Write docs|todo|2",
  "2|Ship typed val diagnostics|doing|5",
  "3|Cut release|done|1",
  "4|Broken row|blocked|3",
];

const tasks: Task[] = [];
for (const row of rows) {
  const parsed = parseTask(row);
  const line = match (parsed) {
    Ok(value: task) => {
      tasks.push(task);
      return describe(task);
    },
    Err(error: message) => `skip: ${message}`,
  };
  console.log(line);
}

val const snapshot = tasks.slice();
console.log(`total estimate: ${totalEstimate(snapshot)}h`);
