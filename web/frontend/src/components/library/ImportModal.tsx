import { useState, useRef, useEffect } from 'react';
import { Upload, FileJson, AlertCircle, CheckCircle2, Loader2 } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { Modal } from '../common/Modal';
import type { ChainDefinitionInput, ChainElement, ChainConnection } from '../../api/types';

interface ImportModalProps {
  isOpen: boolean;
  onClose: () => void;
}

type DetectedType = 'operation' | 'chain' | 'unknown';

interface ParsedContent {
  type: DetectedType;
  name: string;
  category: string;
  description: string;
  raw: Record<string, unknown>;
}

export function ImportModal({ isOpen, onClose }: ImportModalProps) {
  const { send, createChain, state, clearOpDefStatus, clearChainStatus } = useApp();
  const fileInputRef = useRef<HTMLInputElement>(null);

  const [content, setContent] = useState('');
  const [parsed, setParsed] = useState<ParsedContent | null>(null);
  const [parseError, setParseError] = useState<string | null>(null);
  const [isImporting, setIsImporting] = useState(false);
  const [importSuccess, setImportSuccess] = useState(false);

  //
  // Track import status.
  //
  const opDefSuccess = state.opDefSuccess;
  const opDefError = state.opDefError;
  const chainSuccess = state.chains.chainSuccess;
  const chainError = state.chains.chainError;

  //
  // Handle import completion.
  //
  useEffect(() => {
    if (isImporting) {
      if (opDefSuccess || chainSuccess) {
        setIsImporting(false);
        setImportSuccess(true);
        clearOpDefStatus();
        clearChainStatus();
        send({ type: 'op_def_list' });
        window.setTimeout(() => {
          onClose();
          setContent('');
          setParsed(null);
          setImportSuccess(false);
        }, 1500);
      }
      if (opDefError || chainError) {
        setIsImporting(false);
        setParseError(opDefError || chainError || 'Import failed');
        clearOpDefStatus();
        clearChainStatus();
      }
    }
  }, [isImporting, opDefSuccess, opDefError, chainSuccess, chainError, clearOpDefStatus, clearChainStatus, send, onClose]);

  //
  // Reset state when modal opens/closes.
  //
  useEffect(() => {
    if (!isOpen) {
      setContent('');
      setParsed(null);
      setParseError(null);
      setIsImporting(false);
      setImportSuccess(false);
    }
  }, [isOpen]);

  //
  // Parse content when it changes.
  //
  useEffect(() => {
    if (!content.trim()) {
      setParsed(null);
      setParseError(null);
      return;
    }

    try {
      const data = JSON.parse(content);
      const detected = detectType(data);
      setParsed(detected);
      setParseError(null);
    } catch {
      setParsed(null);
      setParseError('Invalid JSON format');
    }
  }, [content]);

  const detectType = (data: Record<string, unknown>): ParsedContent => {
    //
    // Check for explicit item_type field.
    //
    if (data.item_type === 'operation') {
      return {
        type: 'operation',
        name: (data.name as string) || 'Unnamed Operation',
        category: (data.category as string) || 'imported',
        description: (data.description as string) || '',
        raw: data,
      };
    }

    if (data.item_type === 'chain') {
      return {
        type: 'chain',
        name: (data.name as string) || 'Unnamed Chain',
        category: (data.category as string) || 'imported',
        description: (data.description as string) || '',
        raw: data,
      };
    }

    //
    // Auto-detect based on structure.
    // Chains have "elements" array.
    //
    if (Array.isArray(data.elements)) {
      return {
        type: 'chain',
        name: (data.name as string) || 'Unnamed Chain',
        category: (data.category as string) || 'imported',
        description: (data.description as string) || '',
        raw: data,
      };
    }

    //
    // Operations have "operation_prompt" field.
    //
    if (typeof data.operation_prompt === 'string') {
      return {
        type: 'operation',
        name: (data.name as string) || 'Unnamed Operation',
        category: (data.category as string) || 'imported',
        description: (data.description as string) || '',
        raw: data,
      };
    }

    //
    // Unknown type.
    //
    return {
      type: 'unknown',
      name: (data.name as string) || 'Unknown',
      category: (data.category as string) || '',
      description: (data.description as string) || '',
      raw: data,
    };
  };

  const handleFileSelect = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;

    try {
      const text = await file.text();
      setContent(text);
    } catch {
      setParseError('Failed to read file');
    }

    //
    // Reset file input so the same file can be selected again.
    //
    event.target.value = '';
  };

  const handleImport = () => {
    if (!parsed || parsed.type === 'unknown') return;

    setIsImporting(true);
    setParseError(null);

    if (parsed.type === 'operation') {
      //
      // Import operation via op_def_add.
      //
      send({ type: 'op_def_add', content });
    } else {
      //
      // Import chain via chain_create.
      //
      const chainInput: ChainDefinitionInput = {
        name: parsed.raw.name as string,
        description: (parsed.raw.description as string) || '',
        category: (parsed.raw.category as string) || 'imported',
        elements: (parsed.raw.elements as ChainElement[]) || [],
        connections: (parsed.raw.connections as ChainConnection[]) || [],
        disabled: (parsed.raw.disabled as boolean) || false,
        timeout: (parsed.raw.timeout as number) || undefined,
      };
      createChain(chainInput);
    }
  };

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="Import JSON" size="lg">
      <div className="space-y-4">
        {/*
        //
        // File picker.
        //
        */}
        <div className="flex gap-3">
          <button
            onClick={() => fileInputRef.current?.click()}
            className="inline-flex items-center gap-2 px-4 py-2 text-sm border border-subtle hover:bg-[var(--bg-tertiary)] transition-colors"
          >
            <Upload size={16} />
            Choose File
          </button>
          <input
            ref={fileInputRef}
            type="file"
            accept=".json"
            onChange={handleFileSelect}
            className="hidden"
          />
          <span className="text-muted text-sm self-center">or paste JSON below</span>
        </div>

        {/*
        //
        // JSON textarea.
        //
        */}
        <div>
          <label className="block text-sm font-medium mb-1">JSON Content</label>
          <textarea
            value={content}
            onChange={(e) => setContent(e.target.value)}
            disabled={isImporting}
            rows={10}
            placeholder={'{\n  "item_type": "operation",\n  "name": "My Operation",\n  ...\n}'}
            className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm font-mono focus:outline-none focus:border-[var(--border-active)] disabled:opacity-50"
          />
        </div>

        {/*
        //
        // Parse status.
        //
        */}
        {parseError && (
          <div className="flex items-center gap-2 p-3 bg-[var(--accent-error)]/10 text-[var(--accent-error)] text-sm">
            <AlertCircle size={16} />
            {parseError}
          </div>
        )}

        {importSuccess && (
          <div className="flex items-center gap-2 p-3 bg-[var(--accent-success)]/10 text-[var(--accent-success)] text-sm">
            <CheckCircle2 size={16} />
            Import successful!
          </div>
        )}

        {parsed && !parseError && !importSuccess && (
          <div className={`p-3 border text-sm ${
            parsed.type === 'unknown'
              ? 'bg-[var(--accent-warning)]/10 border-[var(--accent-warning)]/30'
              : 'bg-[var(--accent-info)]/10 border-[var(--accent-info)]/30'
          }`}>
            <div className="flex items-center gap-2 mb-2">
              <FileJson size={16} className={
                parsed.type === 'unknown'
                  ? 'text-[var(--accent-warning)]'
                  : 'text-[var(--accent-info)]'
              } />
              <span className="font-medium">
                Detected: {parsed.type === 'unknown' ? 'Unknown Type' : parsed.type === 'operation' ? 'Operation' : 'Chain'}
              </span>
            </div>
            <div className="space-y-1 text-muted">
              <p><span className="font-medium">Name:</span> {parsed.name}</p>
              <p><span className="font-medium">Category:</span> {parsed.category || 'N/A'}</p>
              {parsed.description && (
                <p><span className="font-medium">Description:</span> {parsed.description}</p>
              )}
            </div>
            {parsed.type === 'unknown' && (
              <p className="mt-2 text-[var(--accent-warning)]">
                Could not detect type. Add "item_type": "operation" or "item_type": "chain" to the JSON.
              </p>
            )}
          </div>
        )}

        {/*
        //
        // Actions.
        //
        */}
        <div className="flex justify-end gap-3 pt-2">
          <button
            onClick={onClose}
            disabled={isImporting}
            className="px-4 py-2 text-sm border border-subtle hover:bg-[var(--bg-tertiary)] transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            onClick={handleImport}
            disabled={!parsed || parsed.type === 'unknown' || isImporting || importSuccess}
            className="inline-flex items-center gap-2 px-4 py-2 text-sm bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors disabled:opacity-50"
          >
            {isImporting && <Loader2 size={16} className="animate-spin" />}
            <Upload size={16} />
            {isImporting ? 'Importing...' : 'Import'}
          </button>
        </div>
      </div>
    </Modal>
  );
}
