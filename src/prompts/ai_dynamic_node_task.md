Dynamic node metadata:
- title: {title}
- dependsOn: {depends_on}
- upstream completed nodes: {completed_nodes}

Every worker node must finish with the `dynamic-node-completion` artifact. Use `next.type="end"` when this chain has no more work, `single` for one successor, or `fanout` for parallel branches. Runtime, not you, materializes proposals.
