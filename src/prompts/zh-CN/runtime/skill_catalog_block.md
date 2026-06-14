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

<skill_instructions>
{{#each skills}}

### {{name}}
{{body}}
{{/each}}
</skill_instructions>
{{/if}}
