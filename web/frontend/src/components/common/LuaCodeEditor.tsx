import { CodeEditor } from './CodeEditor';

interface LuaCodeEditorProps {
  value: string;
  onChange?: (value: string) => void;
  readOnly?: boolean;
}

export function LuaCodeEditor({ value, onChange, readOnly }: LuaCodeEditorProps) {
  return <CodeEditor value={value} onChange={onChange} readOnly={readOnly} language="lua" />;
}
