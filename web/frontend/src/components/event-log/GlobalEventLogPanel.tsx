import { useState } from 'react';
import { useApp } from '../../context/AppContext';
import { ApplicationLogTab } from './ApplicationLogTab';

export function GlobalEventLogPanel() {
  const { state } = useApp();
  const nodes = state.systemState?.nodes ?? [];
  //
  // Start with 'all' to show all nodes by default.
  //
  const [selectedNodeId, setSelectedNodeId] = useState<string>('all');

  return (
    <div className="h-full flex flex-col p-4">
      {/*
      //
      // Event log content with node selector integrated.
      //
      */}
      <ApplicationLogTab
        nodeId={selectedNodeId === 'all' ? null : selectedNodeId}
        nodes={nodes}
        selectedNodeId={selectedNodeId}
        onNodeChange={setSelectedNodeId}
      />
    </div>
  );
}
