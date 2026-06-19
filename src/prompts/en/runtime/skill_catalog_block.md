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

To use a Skill, read its SKILL.md file from the listed location. Paths inside a Skill resolve relative to that Skill's directory.
{{/if}}
