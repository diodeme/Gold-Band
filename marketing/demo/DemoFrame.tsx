import type { ReactNode, Ref } from 'react';
import { useTranslation } from 'react-i18next';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import './demo.css';

export function DemoFrame({ children, clientRef }: { children: ReactNode; clientRef: Ref<HTMLDivElement> }) {
  const { t } = useTranslation();
  return <div className="demo-stage">
    <div className="demo-window">
      <ResizablePanelGroup orientation="horizontal" id="demo-window-frame">
        <ResizablePanel id="demo-left-margin" defaultSize="2%" minSize={0} />
        <ResizableHandle aria-label={t('demo.resizeWindow')} className="demo-window-edge" />
        <ResizablePanel id="demo-client" defaultSize="96%" minSize="320px">
          <div ref={clientRef} className="demo-client" data-demo-client="true">{children}</div>
        </ResizablePanel>
        <ResizableHandle aria-label={t('demo.resizeWindow')} className="demo-window-edge" />
        <ResizablePanel id="demo-right-margin" defaultSize="2%" minSize={0} />
      </ResizablePanelGroup>
    </div>
  </div>;
}
