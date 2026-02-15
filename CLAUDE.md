# Project slop

A place to house random AI assisted experiments. Cool stuff may migrate away from here into standalone projects.

## Resources

### ./vendor

Here we will store clones of project source that we can use to read about and gain deep understandings about a third-party dependencies implementation.

From vendor:
- Read this for very thorough understandings about how deps, libs, etc. work
  - Sometimes the source is better than the API doc
- Feel free to ALSO use the web for fast/quick API references and help
- Look at examples directories in vendor for concrete implementation ideas
- Understand what optimizations are done in one language to give ideas of how to do so in another language
  - ie: Project Foobar a C library uses <some_technique> methods to optimize <some_implementation> we can make similar optimizations in Rust

### ./context

Here we will store (for long term) context / hints / API facts we discover that help us build better software.

Dos:
- If we discover in vendor/ or on the internet that an API/Library is best for <some_task> we can compact the info and store that in `./context/<lib_name>.md`
- If we discover that a particular algorithm is known as a great way to highly optimize function/method/etc we compact this info and store that in `./context/aglo.md`
  - Example: https://en.wikipedia.org/wiki/Fast_inverse_square_root we can store information about fast square root in the algo.md
- If we discover great generic design patterns we can store those in `./context/patterns.md`
- If we discover library specific design patterns we can store those in `./context/<lib_name>.md`
- We can also store compact library use info (and function signatures) in `./context/<lib_name>.md` for super fast retrieval when developing

DONTS:
- Dont store private, trademarked, etc information
- Dont store full source code! Short algorithm explanations, math formulas, and architecture ides are all fine


NOTE: We want this informaiton to be dense! DONT INCLUDE FLUFF and try to compact it a bit BUT DONT compact so much we lose details!

#### ./context/pitfalls.md

As we work through bugs and issues in sub-projects IF we encounter pitfalls, errors, bugs, etc
!!ESPECIALLY if we take multiple tries to fix!! Make take some short notes about the issue in `./context/pitfalls.md`.
Use 200 words or so to describe the pitfall and how to avoid.
You can add some info about which sub-project we discovered it in.

Use a template like
```
# <pitfall_title#
<200_words_or_so_to_describe_issue>
<200_words_or_so_to_describe_how_to_avoid>
## Sources
- <sub_project_one>: <short_filename> (optionally: <function_name>)
- <sub_project_two>: <short_filename> (optionally: <function_name>)
```

#### ./context/high_level.md

In `./context/high_level.md` store very high level and short explanations about deps we use and differences or why they might be better than various alternatives

Feel free to build short pros/cons tables of competing libs! ITS okay to mark which ones we use in sub-folder / projects BUT KEEP THE MENTION short!
ie: `ash` used in <sub-project-one> and <sub-project-two>
