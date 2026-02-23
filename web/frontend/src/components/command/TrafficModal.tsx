import { useState, useEffect } from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { Modal } from '../common/Modal';
import { useApp } from '../../context/AppContext';
import {
  ScrollableTrafficTable,
  TrafficFilterBar,
  countTrafficEntries,
  type ProtocolFilter,
} from '../traffic/TrafficTable';
import type { TrafficLogFilters } from '../../api/types';

const DISPLAY_LIMIT = 100;
const FETCH_LIMIT = 10000;

interface TrafficModalProps {
  onClose: () => void;
  fixedNodeId?: string;
}

export function TrafficModal({ onClose, fixedNodeId }: TrafficModalProps) {
  const { state, requestTrafficLog } = useApp();

  const [filters, setFilters] = useState<TrafficLogFilters>({
    node_id: fixedNodeId ?? null,
    agent_short_name: null,
    start_time: null,
    end_time: null,
    url_pattern: null,
    direction: null,
    limit: FETCH_LIMIT,
    offset: 0,
  });
  const [protocolFilter, setProtocolFilter] = useState<ProtocolFilter>('all');
  const [searchFilter, setSearchFilter] = useState('');

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { requestTrafficLog(filters); }, []);

  const handleFilterChange = (newFilters: TrafficLogFilters) => {
    setFilters(newFilters);
    requestTrafficLog(newFilters);
  };

  const handleRefresh = () => {
    requestTrafficLog(filters);
  };

  const handlePrevPage = () => {
    const newOffset = Math.max(0, filters.offset - filters.limit);
    const newFilters = { ...filters, offset: newOffset };
    setFilters(newFilters);
    requestTrafficLog(newFilters);
  };

  const handleNextPage = () => {
    const newOffset = filters.offset + filters.limit;
    if (newOffset < state.intercept.trafficTotalCount) {
      const newFilters = { ...filters, offset: newOffset };
      setFilters(newFilters);
      requestTrafficLog(newFilters);
    }
  };

  const currentPage = Math.floor(filters.offset / filters.limit) + 1;
  const totalPages = Math.ceil(state.intercept.trafficTotalCount / filters.limit);
  const hasPrev = filters.offset > 0;
  const hasNext = filters.offset + filters.limit < state.intercept.trafficTotalCount;

  return (
    <Modal
      isOpen={true}
      onClose={onClose}
      title="Intercepted Traffic"
      size="full"
      noPadding
    >
      <div className="flex flex-col h-[75vh] p-4 gap-3">
        <TrafficFilterBar
          filters={filters}
          setFilters={handleFilterChange}
          protocolFilter={protocolFilter}
          setProtocolFilter={setProtocolFilter}
          searchFilter={searchFilter}
          setSearchFilter={setSearchFilter}
          onRefresh={handleRefresh}
        />

        <ScrollableTrafficTable
          entries={state.intercept.trafficLog}
          protocolFilter={protocolFilter}
          searchFilter={searchFilter}
          expandedRow={null}
          setExpandedRow={() => {}}
          showNodeColumn={!fixedNodeId}
          displayLimit={DISPLAY_LIMIT}
          heightMode="flex"
          emptyMessage="No intercepted traffic"
        />

        {state.intercept.trafficTotalCount > 0 && (
          <div className="flex items-center justify-between text-xs flex-shrink-0">
            <div className="text-muted">
              Showing {Math.min(countTrafficEntries(state.intercept.trafficLog, protocolFilter, searchFilter), DISPLAY_LIMIT)} of {state.intercept.trafficTotalCount}
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={handlePrevPage}
                disabled={!hasPrev}
                className="flex items-center gap-1 px-3 py-1 text-muted hover:text-title border border-subtle transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <ChevronLeft size={12} /> PREV
              </button>
              <span className="text-muted px-2">{currentPage} / {totalPages || 1}</span>
              <button
                onClick={handleNextPage}
                disabled={!hasNext}
                className="flex items-center gap-1 px-3 py-1 text-muted hover:text-title border border-subtle transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                NEXT <ChevronRight size={12} />
              </button>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}
