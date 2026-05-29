import React, { useState, useRef, useEffect } from 'react';
import { Input, Button, Space, Empty, Spin, Avatar, Typography } from 'antd';
import type { TextAreaRef } from 'antd/es/input/TextArea';
import { SendOutlined, UserOutlined, RobotOutlined, ClearOutlined, ThunderboltOutlined } from '@ant-design/icons';
import type { ChatMessage } from '@/types';
import { useChatStore } from '@/stores';
import MessageContentRenderer from './MessageContentRenderer';
import styles from './Chat.module.css';

const { TextArea } = Input;
const { Text } = Typography;

const Chat: React.FC = () => {
  const { messages, streaming, sendMessage, stopStreaming, clearMessages } = useChatStore();
  const [inputValue, setInputValue] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<TextAreaRef>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const handleSend = async () => {
    if (!inputValue.trim() || streaming) return;
    
    const text = inputValue.trim();
    setInputValue('');
    await sendMessage({ content: text });
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const renderMessage = (msg: ChatMessage) => {
    const isUser = msg.role === 'user';
    const isSystem = msg.role === 'system';

    return (
      <div
        key={msg.id}
        className={`${styles.messageWrapper} ${isUser ? styles.userMessage : styles.assistantMessage}`}
      >
        <div className={styles.messageAvatar}>
          <Avatar
            icon={isUser ? <UserOutlined /> : <RobotOutlined />}
            style={{ backgroundColor: isUser ? '#1890ff' : '#52c41a' }}
          />
        </div>
        <div className={styles.messageContent}>
          <div className={styles.messageHeader}>
            <Text strong>{isUser ? '我' : isSystem ? '系统' : 'Agent'}</Text>
            <Text type="secondary" className={styles.messageTime}>
              {new Date(msg.createdAt).toLocaleTimeString()}
            </Text>
          </div>
          <div className={styles.messageBody}>
            <MessageContentRenderer content={msg.content} role={msg.role} />
          </div>
        </div>
      </div>
    );
  };

  return (
    <div className={styles.container}>
      <div className={styles.messagesContainer}>
        {messages.length === 0 ? (
          <div className={styles.emptyState}>
            <Empty
              image={<RobotOutlined style={{ fontSize: 64, color: '#d9d9d9' }} />}
              description={
                <span>
                  开始与 Agent 对话
                  <br />
                  <Text type="secondary">Agent 会通过 SA 协调分析您的需求</Text>
                </span>
              }
            />
          </div>
        ) : (
          <>
            {messages.map(renderMessage)}
            {streaming && (
              <div className={`${styles.messageWrapper} ${styles.assistantMessage}`}>
                <div className={styles.messageAvatar}>
                  <Avatar icon={<RobotOutlined />} style={{ backgroundColor: '#52c41a' }} />
                </div>
                <div className={styles.messageContent}>
                  <div className={styles.messageHeader}>
                    <Text strong>Agent</Text>
                  </div>
                  <div className={styles.messageBody}>
                    <div className={styles.thinkingIndicator}>
                      <ThunderboltOutlined style={{ color: '#722ed1' }} />
                      <span>SA 正在协调分析，PDCA 循环执行中...</span>
                      <Spin size="small" />
                    </div>
                  </div>
                </div>
              </div>
            )}
            <div ref={messagesEndRef} />
          </>
        )}
      </div>

      <div className={styles.inputArea}>
        <TextArea
          ref={inputRef}
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="输入消息... (Enter 发送, Shift+Enter 换行)"
          autoSize={{ minRows: 1, maxRows: 4 }}
          disabled={streaming}
          className={styles.input}
        />
        <Space>
          <Button
            icon={<ClearOutlined />}
            onClick={clearMessages}
            disabled={messages.length === 0 || streaming}
          >
            清空
          </Button>
          <Button
            type="primary"
            icon={<SendOutlined />}
            onClick={handleSend}
            loading={streaming}
            disabled={!inputValue.trim()}
          >
            发送
          </Button>
        </Space>
      </div>
    </div>
  );
};

export default Chat;
