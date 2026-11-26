// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useMemo, useRef, type ReactElement, type RefObject } from 'react';
import {
  Background,
  BackgroundVariant,
  Panel,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  type Node,
  type NodeProps,
  type NodeTypes,
  type Viewport,
} from '@xyflow/react';
import '@xyflow/react/dist/base.css';
import type { AgentView } from '@/lib/contract';
import { MAX_ZOOM, MIN_ZOOM, ROW_HEIGHT, boardWidth, fitViewport, initialViewport, layoutRows } from '@/lib/canvas';
import type { FilterKey } from '@/lib/filters';
import { useMessages } from '@/hooks/use-messages';
import { BoardRow } from './BoardRow';
import { StrokeIcon } from './StrokeIcon';

interface RowNodeData extends Record<string, unknown> {
  agent: AgentView;
  filter: FilterKey;
  now: number;
  onOpenAgent: (agentId: string) => void;
}

type RowNode = Node<RowNodeData, 'row'>;

function RowNodeView({ data }: NodeProps<RowNode>): ReactElement {
  return <BoardRow agent={data.agent} filter={data.filter} now={data.now} onOpenAgent={data.onOpenAgent} />;
}

const nodeTypes: NodeTypes = { row: RowNodeView };

/** Where the board was left: opening an agent unmounts the canvas, coming back puts the view where it was. */
let savedViewport: Viewport | null = null;

interface CanvasControlsProps {
  containerRef: RefObject<HTMLDivElement | null>;
}

function CanvasControls({ containerRef }: CanvasControlsProps): ReactElement {
  const messages = useMessages();
  const { zoomIn, zoomOut, getNodes, getNodesBounds, setViewport } = useReactFlow();
  const showAll = (): void => {
    const container = containerRef.current;
    if (!container) return;
    void setViewport(fitViewport(getNodesBounds(getNodes()), container.clientWidth, container.clientHeight), { duration: 200 });
  };
  const buttonClass =
    'flex size-10 items-center justify-center rounded-[10px] text-subtle transition-colors hover:bg-secondary hover:text-foreground';
  return (
    <Panel position="bottom-left" className="!m-4 flex gap-0.5 rounded-[12px] border bg-background p-1">
      <button type="button" aria-label={messages.board.zoomIn} className={buttonClass} onClick={() => void zoomIn({ duration: 150 })}>
        <StrokeIcon size={14} strokeWidth={1.4}>
          <path d="M7 2v10M2 7h10" />
        </StrokeIcon>
      </button>
      <button type="button" aria-label={messages.board.zoomOut} className={buttonClass} onClick={() => void zoomOut({ duration: 150 })}>
        <StrokeIcon size={14} strokeWidth={1.4}>
          <path d="M2 7h10" />
        </StrokeIcon>
      </button>
      <button type="button" aria-label={messages.board.showAll} className={buttonClass} onClick={showAll}>
        <StrokeIcon size={14} strokeWidth={1.4}>
          <path d="M2 5V2h3M9 2h3v3M12 9v3H9M5 12H2V9" />
        </StrokeIcon>
      </button>
    </Panel>
  );
}

interface BoardCanvasProps {
  agents: readonly AgentView[];
  filter: FilterKey;
  now: number;
  onOpenAgent: (agentId: string) => void;
}

/** The board as a canvas, one row per agent: pan by dragging or scrolling, zoom by pinch or ctrl + wheel; "Show all" fits it all. */
export function BoardCanvas({ agents, filter, now, onOpenAgent }: BoardCanvasProps): ReactElement {
  const messages = useMessages();
  const nodes = useMemo<RowNode[]>(() => {
    const layout = layoutRows(agents);
    return agents.map((agent, index) => ({
      id: agent.id,
      type: 'row',
      position: layout[index]?.position ?? { x: 0, y: 0 },
      width: layout[index]?.width ?? 0,
      height: ROW_HEIGHT,
      data: { agent, filter, now, onOpenAgent },
      draggable: false,
      selectable: false,
      connectable: false,
      focusable: false,
    }));
  }, [agents, filter, now, onOpenAgent]);
  const containerRef = useRef<HTMLDivElement>(null);

  return (
    <div ref={containerRef} role="region" aria-label={messages.board.canvasLabel} className="atlas-canvas size-full">
      <ReactFlowProvider>
        <ReactFlow
          nodes={nodes}
          edges={[]}
          nodeTypes={nodeTypes}
          nodesDraggable={false}
          nodesConnectable={false}
          nodesFocusable={false}
          edgesFocusable={false}
          elementsSelectable={false}
          panOnDrag
          panOnScroll
          zoomOnScroll={false}
          zoomOnPinch
          zoomOnDoubleClick={false}
          minZoom={MIN_ZOOM}
          maxZoom={MAX_ZOOM}
          proOptions={{ hideAttribution: true }}
          onInit={(instance) => {
            const contentWidth = boardWidth(agents);
            void instance.setViewport(savedViewport ?? initialViewport(contentWidth, containerRef.current?.clientWidth ?? contentWidth));
          }}
          onMoveEnd={(_event, viewport) => {
            savedViewport = viewport;
          }}
        >
          {/* React Flow's own dot pattern: its gap, size and grey. */}
          <Background variant={BackgroundVariant.Dots} gap={20} size={1} color="var(--board-dot)" bgColor="var(--board)" />
          <CanvasControls containerRef={containerRef} />
        </ReactFlow>
      </ReactFlowProvider>
    </div>
  );
}
