export function getOsType() {
    const platform = navigator.platform?.toLowerCase() || '';
    const userAgent = navigator.userAgent?.toLowerCase() || '';

    if (platform.includes('win') || userAgent.includes('windows')) {
        return 'windows';
    }
    if (platform.includes('mac') || userAgent.includes('macintosh')) {
        return 'macos';
    }
    return 'linux';
}

export function getSetupCommands() {
    const os = getOsType();

    const commands = {
        windows: {
            os: 'Windows',
            terminalName: 'PowerShell',
            install: 'winget install Ollama.Ollama',
            serve: '$env:OLLAMA_ORIGINS="chrome-extension://*"; ollama serve'
        },
        macos: {
            os: 'macOS',
            terminalName: 'Terminal',
            install: 'brew install ollama',
            serve: 'OLLAMA_ORIGINS="chrome-extension://*" ollama serve'
        },
        linux: {
            os: 'Linux',
            terminalName: 'Terminal',
            install: 'curl -fsSL https://ollama.com/install.sh | sh',
            serve: 'OLLAMA_ORIGINS="chrome-extension://*" ollama serve'
        }
    };

    return commands[os] || commands.linux;
}
