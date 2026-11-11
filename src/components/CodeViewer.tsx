// Read-only Monaco viewer. Monaco is bundled locally (no CDN) so it works offline.
import Editor, { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import { useEffect, useRef } from "react";

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };
loader.config({ monaco });

monaco.editor.defineTheme("nexus", {
  base: "vs-dark",
  inherit: true,
  rules: [],
  colors: { "editor.background": "#050506", "editorGutter.background": "#050506", "editor.lineHighlightBackground": "#101013" },
});

const byExtension: Record<string, string> = {
  ts: "typescript", tsx: "typescript", js: "javascript", jsx: "javascript", mjs: "javascript", cjs: "javascript",
  rs: "rust", py: "python", go: "go", java: "java", kt: "kotlin", swift: "swift", c: "c", h: "c", cpp: "cpp",
  cc: "cpp", hpp: "cpp", cs: "csharp", rb: "ruby", php: "php", lua: "lua", luau: "lua", sh: "shell", bash: "shell",
  sql: "sql", html: "html", css: "css", scss: "scss", less: "less", md: "markdown", json: "json", yaml: "yaml",
  yml: "yaml", toml: "ini", xml: "xml", dockerfile: "dockerfile", graphql: "graphql", ps1: "powershell",
};

export default function CodeViewer({ path, value, line }: { path: string; value: string; line?: number }) {
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const name = path.split("/").pop()?.toLowerCase() ?? "";
  const language = byExtension[name.includes(".") ? name.split(".").pop()! : name] ?? "plaintext";

  useEffect(() => {
    if (editor.current && line) {
      editor.current.revealLineInCenter(line);
      editor.current.setPosition({ lineNumber: line, column: 1 });
    }
  }, [line, value]);

  return (
    <Editor
      path={path}
      value={value}
      language={language}
      theme="nexus"
      onMount={(e) => {
        editor.current = e;
        if (line) {
          e.revealLineInCenter(line);
          e.setPosition({ lineNumber: line, column: 1 });
        }
      }}
      options={{
        readOnly: true,
        minimap: { enabled: false },
        fontSize: 12.5,
        scrollBeyondLastLine: false,
        renderLineHighlight: "line",
        automaticLayout: true,
      }}
    />
  );
}
