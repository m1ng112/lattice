//! Stream processing primitives and inter-node channels.

use crate::node::Value;
use tokio::sync::mpsc;

/// A bounded channel for passing data between nodes in the dataflow graph.
///
/// Uses [`tokio::sync::mpsc`] to provide backpressure when the receiver
/// cannot keep up with the sender.
pub struct DataChannel {
    pub sender: mpsc::Sender<Value>,
    pub receiver: mpsc::Receiver<Value>,
}

impl DataChannel {
    /// Create a new bounded data channel with the given buffer size.
    ///
    /// A smaller buffer provides stronger backpressure; a larger buffer
    /// allows more pipelining between producer and consumer nodes.
    pub fn new(buffer_size: usize) -> Self {
        let (sender, receiver) = mpsc::channel(buffer_size);
        Self { sender, receiver }
    }
}

/// Apply a transformation function to each value in the collection.
pub fn map_values(values: Vec<Value>, f: impl Fn(&Value) -> Value) -> Vec<Value> {
    values.iter().map(f).collect()
}

/// Retain only values that satisfy the predicate.
pub fn filter_values(values: Vec<Value>, pred: impl Fn(&Value) -> bool) -> Vec<Value> {
    values.into_iter().filter(|v| pred(v)).collect()
}

/// Fold (reduce) a collection of values into an accumulator.
pub fn fold_values<A>(values: Vec<Value>, init: A, f: impl Fn(A, &Value) -> A) -> A {
    values.iter().fold(init, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_doubles_ints() {
        let values = vec![Value::Int(1), Value::Int(2), Value::Int(3)];
        let doubled = map_values(values, |v| match v {
            Value::Int(n) => Value::Int(n * 2),
            other => other.clone(),
        });
        assert_eq!(
            doubled,
            vec![Value::Int(2), Value::Int(4), Value::Int(6)]
        );
    }

    #[test]
    fn filter_positive_ints() {
        let values = vec![Value::Int(-1), Value::Int(0), Value::Int(1), Value::Int(2)];
        let positive = filter_values(values, |v| {
            matches!(v, Value::Int(n) if *n > 0)
        });
        assert_eq!(positive, vec![Value::Int(1), Value::Int(2)]);
    }

    #[test]
    fn fold_sum() {
        let values = vec![Value::Int(1), Value::Int(2), Value::Int(3)];
        let sum = fold_values(values, 0i64, |acc, v| {
            acc + v.as_int().unwrap_or(0)
        });
        assert_eq!(sum, 6);
    }

    #[tokio::test]
    async fn channel_send_receive() {
        let DataChannel { sender, mut receiver } = DataChannel::new(4);
        sender.send(Value::Int(42)).await.unwrap();
        sender.send(Value::String("hello".into())).await.unwrap();
        drop(sender);

        let v1 = receiver.recv().await.unwrap();
        let v2 = receiver.recv().await.unwrap();
        let v3 = receiver.recv().await;

        assert_eq!(v1, Value::Int(42));
        assert_eq!(v2, Value::String("hello".into()));
        assert!(v3.is_none());
    }

    #[tokio::test]
    async fn channel_backpressure() {
        // Buffer of 1 means sender blocks after first send until receiver reads
        let DataChannel { sender, mut receiver } = DataChannel::new(1);

        let send_handle = tokio::spawn(async move {
            sender.send(Value::Int(1)).await.unwrap();
            // This would block if buffer is full and no receiver
            sender.send(Value::Int(2)).await.unwrap();
        });

        let v1 = receiver.recv().await.unwrap();
        let v2 = receiver.recv().await.unwrap();
        send_handle.await.unwrap();

        assert_eq!(v1, Value::Int(1));
        assert_eq!(v2, Value::Int(2));
    }
}
