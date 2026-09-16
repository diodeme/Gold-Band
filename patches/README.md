# Compatibility dependency patches

`mdast-util-gfm-autolink-literal+2.0.1.patch` removes a native RegExp lookbehind from the GFM email fallback transform. The package already validates the preceding boundary through its `previous(match, true)` helper, so removing the redundant lookbehind preserves the upstream AST semantics while allowing WebKit 613 to parse the module.

Upstream package: <https://github.com/syntax-tree/mdast-util-gfm-autolink-literal>

Remove this patch as soon as an upstream release no longer contains the lookbehind and the Gold Band Markdown compatibility tests pass against that release.
