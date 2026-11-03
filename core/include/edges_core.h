#ifndef EDGES_CORE_H
#define EDGES_CORE_H

/* The whole bridge between an edge and the core.
 *
 * Pass a JSON command such as {"command":"open","url":"edges://projects"}.
 * Returns {"ok": ...} or {"error": {"kind": ..., "message": ...}} as an owned
 * UTF-8 string that must be released exactly once with edges_free.
 * Commands that reach Tuist block, so call this off the main thread.
 */
char *edges_call(const char *input);
void edges_free(char *output);

#endif
