{{#if has_skills}}
## Agent Skills

You have access to the following Skills — modular capabilities that provide specialized instructions for specific tasks.

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
