
use crate::memory::l1_session::cosine_similarity;

/// 实时相关性跟踪器
///
/// 每次 Agent 收到新输入时（包括初始任务输入和中间补充输入）:
/// 1. 计算输入 embedding
/// 2. 与任务 5W2H embedding 对比 → 全局任务相关度
/// 3. 与上一轮输入 embedding 对比 → 局部连贯性
/// 4. 输出融合后的 relevance_score
///
/// score = α * cosine_sim(input_emb, task_emb)
///       + (1-α) * cosine_sim(input_emb, prev_input_emb)
pub struct RelevanceTracker {
    /// 任务 5W2H 的语义 embedding（从 what+why 生成）
    task_5w2h_embedding: Option<Vec<f32>>,
    /// 上一轮输入的 embedding
    prev_input_embedding: Option<Vec<f32>>,
    /// 全局权重: α * global + (1-α) * local
    alpha: f64,
}

impl RelevanceTracker {
    pub fn new(alpha: f64) -> Self {
        Self {
            task_5w2h_embedding: None,
            prev_input_embedding: None,
            alpha,
        }
    }

    /// 收到新输入时调用
    ///
    /// # Arguments
    /// * `embedding` - 由外部 EmbeddingService 生成
    ///
    /// # Returns
    /// relevance_score [0, 1]
    pub fn on_new_input(&mut self, embedding: &[f32]) -> f64 {
        // 全局任务相关度
        let global_score = self
            .task_5w2h_embedding
            .as_ref()
            .map(|task_emb| cosine_similarity(embedding, task_emb).abs().max(0.001))
            .unwrap_or(0.5);

        // 局部连贯性
        let local_score = self
            .prev_input_embedding
            .as_ref()
            .map(|prev_emb| cosine_similarity(embedding, prev_emb).abs().max(0.001))
            .unwrap_or(0.5);

        self.prev_input_embedding = Some(embedding.to_vec());

        self.alpha * global_score + (1.0 - self.alpha) * local_score
    }

    /// 设置/更新任务 5W2H embedding
    pub fn set_task_context(&mut self, task_embedding: Vec<f32>) {
        self.task_5w2h_embedding = Some(task_embedding);
    }

    /// 获取上一轮输入 embedding
    pub fn get_prev_embedding(&self) -> Option<&Vec<f32>> {
        self.prev_input_embedding.as_ref()
    }

    /// 获取任务 5W2H embedding
    pub fn get_task_embedding(&self) -> Option<&Vec<f32>> {
        self.task_5w2h_embedding.as_ref()
    }

    /// 重置（新任务时调用）
    pub fn reset(&mut self) {
        self.task_5w2h_embedding = None;
        self.prev_input_embedding = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_emb(v: Vec<f64>) -> Vec<f32> {
        v.into_iter().map(|x| x as f32).collect()
    }

    #[test]
    fn test_new_input_no_task_context() {
        let mut tracker = RelevanceTracker::new(0.6);
        // 无 task context, 无 prev input → fallback
        let score = tracker.on_new_input(&make_emb(vec![1.0, 0.0, 0.0]));
        assert!((score - 0.5).abs() < 0.001, "fallback should be 0.5, got {}", score);
    }

    #[test]
    fn test_new_input_with_task_context() {
        let mut tracker = RelevanceTracker::new(0.6);
        tracker.set_task_context(make_emb(vec![1.0, 0.0, 0.0]));

        // input matches task → high global score
        let score = tracker.on_new_input(&make_emb(vec![0.99, 0.01, 0.01]));
        assert!(score > 0.5, "matching input should score > 0.5, got {}", score);
    }

    #[test]
    fn test_global_weighting() {
        let mut tracker = RelevanceTracker::new(1.0); // α=1 → 只看全局
        tracker.set_task_context(make_emb(vec![1.0, 0.0]));

        let score = tracker.on_new_input(&make_emb(vec![1.0, 0.0]));
        assert!((score - 1.0).abs() < 0.01, "exact match with α=1 should give 1.0, got {}", score);
    }

    #[test]
    fn test_local_coherence() {
        let mut tracker = RelevanceTracker::new(0.0); // α=0 → 只看局部连贯性
        tracker.set_task_context(make_emb(vec![1.0, 0.0]));

        // 第一个 input: 有 task context, 无 prev → local fallback 0.5
        let first = tracker.on_new_input(&make_emb(vec![0.5, 0.5]));
        assert!((first - 0.5).abs() < 0.001, "first input with α=0 should be 0.5, got {}", first);

        // 第二个 input: 与第一个一致 → high local score
        let second = tracker.on_new_input(&make_emb(vec![0.5, 0.5]));
        assert!((second - 1.0).abs() < 0.01, "identical consecutive inputs with α=0 should give 1.0, got {}", second);
    }

    #[test]
    fn test_reset() {
        let mut tracker = RelevanceTracker::new(0.6);
        tracker.set_task_context(make_emb(vec![1.0, 0.0]));
        let _ = tracker.on_new_input(&make_emb(vec![1.0, 0.0]));

        tracker.reset();
        assert!(tracker.get_task_embedding().is_none());
        assert!(tracker.get_prev_embedding().is_none());
    }
}
