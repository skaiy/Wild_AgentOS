import React, { useEffect, useRef } from 'react';
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import 'xterm/css/xterm.css';
import styles from './TerminalLog.module.css';

interface TerminalLogProps {
  logStream: string;
  autoScroll?: boolean;
  maxLines?: number;
  height?: number;
}

const TerminalLog: React.FC<TerminalLogProps> = ({
  logStream,
  autoScroll = true,
  maxLines = 1000,
  height = 300,
}) => {
  const terminalRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    if (!terminalRef.current) return;

    const term = new Terminal({
      theme: {
        background: '#1e1e1e',
        foreground: '#d4d4d4',
        cursor: '#d4d4d4',
        cursorAccent: '#1e1e1e',
        selectionBackground: 'rgba(255, 255, 255, 0.2)',
        black: '#000000',
        red: '#cd3131',
        green: '#0dbc79',
        yellow: '#e5e510',
        blue: '#2472c8',
        magenta: '#bc3fbc',
        cyan: '#11a8cd',
        white: '#e5e5e5',
        brightBlack: '#666666',
        brightRed: '#f14c4c',
        brightGreen: '#23d18b',
        brightYellow: '#f5f543',
        brightBlue: '#3b8eea',
        brightMagenta: '#d670d6',
        brightCyan: '#29b8db',
        brightWhite: '#e5e5e5',
      },
      fontFamily: 'Consolas, Monaco, monospace',
      fontSize: 13,
      lineHeight: 1.4,
      scrollback: maxLines,
      cursorBlink: false,
      cursorStyle: 'block',
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(terminalRef.current);
    
    setTimeout(() => {
      fitAddon.fit();
    }, 0);

    termRef.current = term;
    fitAddonRef.current = fitAddon;

    const handleResize = () => {
      fitAddon.fit();
    };

    window.addEventListener('resize', handleResize);

    return () => {
      window.removeEventListener('resize', handleResize);
      term.dispose();
    };
  }, [maxLines]);

  useEffect(() => {
    if (!termRef.current || !logStream) return;

    termRef.current.write(logStream);
    
    if (autoScroll) {
      termRef.current.scrollToBottom();
    }
  }, [logStream, autoScroll]);

  useEffect(() => {
    if (fitAddonRef.current) {
      fitAddonRef.current.fit();
    }
  }, [height]);

  return (
    <div
      ref={terminalRef}
      className={styles.terminal}
      style={{ height: `${height}px` }}
    />
  );
};

export default TerminalLog;
