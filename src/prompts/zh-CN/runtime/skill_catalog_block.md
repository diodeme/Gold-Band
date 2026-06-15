{{#if has_skills}}
## Agent Skills

你可以使用以下 Skills — 提供特定任务专业指令的模块化能力。

<available_skills>
{{#each skills}}
  <skill>
    <name>{{name}}</name>
    <description>{{description}}</description>
    <location>{{directory_path}}</location>
  </skill>
{{/each}}
</available_skills>

要使用某个 Skill，请从其列出的位置目录中读取 SKILL.md 文件。Skill 内的路径相对于该 Skill 的目录解析。
{{/if}}
