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
            permanentSetup: {
                title: 'set environment variable (one-time)',
                steps: [
                    'open start → search "environment variables" → click "edit the system environment variables"',
                    'click "environment variables" button',
                    'under "user variables", click "new"',
                    'variable name: OLLAMA_ORIGINS',
                    'variable value: chrome-extension://*',
                    'click ok, then restart ollama from the system tray'
                ],
                note: 'this only needs to be done once. ollama will remember the setting.',
                command: 'setx OLLAMA_ORIGINS "chrome-extension://*"',
                commandNote: 'run this in powershell (one-time, then restart ollama):'
            },
            serve: '$env:OLLAMA_ORIGINS="chrome-extension://*"; ollama serve',
            serveNote: 'Temporary - only for this terminal session'
        },
        macos: {
            os: 'macOS',
            terminalName: 'Terminal',
            install: 'brew install ollama',
            permanentSetup: {
                title: 'set environment variable (one-time)',
                steps: [
                    'open terminal',
                    'run the command below',
                    'quit ollama from the menu bar and relaunch it'
                ],
                command: 'launchctl setenv OLLAMA_ORIGINS "chrome-extension://*"',
                commandNote: 'run this once in terminal:',
                note: 'note: you may need to run this again after a system restart.'
            },
            serve: 'OLLAMA_ORIGINS="chrome-extension://*" ollama serve',
            serveNote: 'Temporary - only for this terminal session'
        },
        linux: {
            os: 'Linux',
            terminalName: 'Terminal',
            install: 'curl -fsSL https://ollama.com/install.sh | sh',
            permanentSetup: {
                title: 'set environment variable (one-time)',
                steps: [
                    'create a systemd override directory and file',
                    'add the environment variable',
                    'reload and restart the service'
                ],
                commands: [
                    'sudo mkdir -p /etc/systemd/system/ollama.service.d',
                    'echo -e "[Service]\\nEnvironment=OLLAMA_ORIGINS=chrome-extension://*" | sudo tee /etc/systemd/system/ollama.service.d/origins.conf',
                    'sudo systemctl daemon-reload && sudo systemctl restart ollama'
                ],
                commandNote: 'run these commands in terminal:',
                note: 'this permanently configures ollama to allow chrome extensions.'
            },
            serve: 'OLLAMA_ORIGINS="chrome-extension://*" ollama serve',
            serveNote: 'Temporary - only for this terminal session'
        }
    };

    return commands[os] || commands.linux;
}
