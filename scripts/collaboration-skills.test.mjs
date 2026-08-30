import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import Ajv from "ajv";
import { parse as parseYaml } from "yaml";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const claudeSkills = path.join(repoRoot, ".claude", "skills");

const skillNames = ["git-issue", "git-pr"];
const issueTemplates = [
  "bug-report.yml",
  "feature-request.yml",
  "performance-issue.yml",
  "technical-proposal.yml",
];

function parseFrontmatter(source) {
  const match = source.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n/);
  assert.ok(match, "SKILL.md must start with YAML frontmatter");

  const fields = new Map();
  for (const line of match[1].split(/\r?\n/)) {
    const separator = line.indexOf(":");
    assert.ok(separator > 0, `invalid frontmatter line: ${line}`);
    fields.set(line.slice(0, separator).trim(), line.slice(separator + 1).trim());
  }
  return fields;
}

for (const skillName of skillNames) {
  test(`${skillName} has valid metadata and a revision-bound review gate`, async () => {
    const canonicalPath = path.join(claudeSkills, skillName, "SKILL.md");
    const canonical = await readFile(canonicalPath, "utf8");

    const frontmatter = parseFrontmatter(canonical);
    assert.deepEqual([...frontmatter.keys()], ["name", "description"]);
    assert.equal(frontmatter.get("name"), skillName);
    assert.ok(frontmatter.get("description")?.length > 40);
    assert.match(canonical, /explicit approval in a later user response/i);
    assert.match(canonical, /Bind approval to the displayed revision/i);

    const metadata = await readFile(
      path.join(claudeSkills, skillName, "agents", "openai.yaml"),
      "utf8",
    );
    assert.match(metadata, new RegExp(`\\$${skillName}\\b`));
  });
}

test("English issue forms cover the four canonical collaboration cases", async () => {
  const [schemaResponse, configSchemaResponse] = await Promise.all([
    fetch("https://json.schemastore.org/github-issue-forms.json"),
    fetch("https://json.schemastore.org/github-issue-config.json"),
  ]);
  assert.equal(schemaResponse.ok, true, "Issue Forms schema must be available");
  assert.equal(
    configSchemaResponse.ok,
    true,
    "Issue template config schema must be available",
  );
  const validateIssueForm = new Ajv({ allErrors: true, strict: false }).compile(
    await schemaResponse.json(),
  );
  const validateIssueConfig = new Ajv({ allErrors: true, strict: false }).compile(
    await configSchemaResponse.json(),
  );
  const templateRoot = path.join(repoRoot, ".github", "ISSUE_TEMPLATE");
  for (const templateName of issueTemplates) {
    const source = await readFile(path.join(templateRoot, templateName), "utf8");
    assert.equal(
      validateIssueForm(parseYaml(source)),
      true,
      `${templateName} violates the GitHub Issue Forms schema: ${JSON.stringify(validateIssueForm.errors)}`,
    );
    assert.match(source, /^name: .+/m);
    assert.match(source, /^description: .+/m);
    assert.match(source, /^body:/m);
    assert.match(source, /Acceptance criteria/);
    assert.doesNotMatch(source, /[\u3400-\u9fff]/u);
  }

  const configSource = await readFile(
    path.join(templateRoot, "config.yml"),
    "utf8",
  );
  assert.equal(
    validateIssueConfig(parseYaml(configSource)),
    true,
    `config.yml violates the GitHub issue template config schema: ${JSON.stringify(validateIssueConfig.errors)}`,
  );
  assert.match(configSource, /^blank_issues_enabled: true$/m);
});

test("the PR template records verification, documentation, and design reviews", async () => {
  const source = await readFile(
    path.join(repoRoot, ".github", "PULL_REQUEST_TEMPLATE.md"),
    "utf8",
  );
  for (const heading of [
    "Summary",
    "Root Cause / Design Rationale",
    "Verification",
    "Documentation",
    "Performance and Overdesign Review",
    "Related Issues",
  ]) {
    assert.match(source, new RegExp(`^## ${heading}$`, "m"));
  }
  assert.doesNotMatch(source, /[\u3400-\u9fff]/u);
});
