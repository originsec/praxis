# Best practices for working with the praxis code

## Code comments

- Source files do not need a comment at the start.
- No need to be overly verbose with comments. If the code is self-explanatory, don't add a comment.
- Comment, rather, larger blocks where it's necessary to understand what's going on.
- Single-line comments should be rare - and mostly used when necessary inline (i.e. same line as code, after the code).
- Comments should be descriptive and explanatory, not just stating an obvious fact.
- Generally, comments should be in the following format (note: newlines before & after):

```
<newline>
//
// Comment here followed by full stop.
// Multiline is ok but wrap to 80 characters. An example is this line you can
// see how I've wrapped it.
//
<newline>
```

## Architecture

- Main components are: Node, Service, Web
- semantic_parser is a component that is eventually to be packaged separately
- Do not re-implement the same code in different components. Favour sharing code.
- Shared code goes in common/
- Even within a component, favour identifying opportunities for shared code and sensible abstractions.
- A sensible abstraction is one where it is likely that there could be an expansion over a single consumer of any subcomponent/interface.

