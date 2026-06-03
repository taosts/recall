import type { QuestSummary } from '../lib/types';

interface QuestBadgeProps {
  quests: QuestSummary[];
  onQuestClick?: (id: string) => void;
}

export function QuestBadge({ quests, onQuestClick }: QuestBadgeProps) {
  if (!quests || quests.length === 0) return null;

  const visible = quests.slice(0, 2);
  const overflow = quests.length - 2;

  return (
    <span style={{ display: 'inline-flex', gap: '4px', flexWrap: 'wrap' }}>
      {visible.map((q) => (
        <span
          key={q.id}
          className="chip badge-quest"
          style={{
            fontSize: '11px',
            padding: '2px 8px',
            cursor: onQuestClick ? 'pointer' : 'default',
          }}
          onClick={(e) => {
            if (onQuestClick) {
              e.stopPropagation();
              onQuestClick(q.id);
            }
          }}
          title={q.display_name}
        >
          {q.display_name}
        </span>
      ))}
      {overflow > 0 && (
        <span
          className="chip badge-quest"
          style={{ fontSize: '11px', padding: '2px 8px', cursor: 'default' }}
        >
          +{overflow} more
        </span>
      )}
    </span>
  );
}
