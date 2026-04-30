import React, { useEffect } from 'react';
import { Box, Text, useInput } from 'ink';
import { centerText } from '../utils/display';

export type PlaybackAPI = {
	play: () => void;
	pause: () => void;
	restart: () => void;
};

export type AsciiMotionCliProps = {
	hasDarkBackground?: boolean;
	autoPlay?: boolean;
	loop?: boolean;
	onReady?: (api: PlaybackAPI) => void;
	onInteraction?: () => void;
};

const NOISE_CHARS = "!@#$%^&*()-_=+[]{}|;:,./<>?~0123456789"
export const NOISE_LINES = Array.from({ length: 3 }, () =>
	Array.from({ length: 28 }, () => NOISE_CHARS[Math.floor(Math.random() * NOISE_CHARS.length)]).join("")
)

export const StaticRobotFrame: React.FC<{ hasDarkBackground?: boolean }> = () => {
	return (
		<Box flexDirection="column" marginBottom={1} marginTop={1}>
			{NOISE_LINES.map((line, idx) => (
				<Text color="gray" key={idx}>{centerText(line)}</Text>
			))}
			<Text color="#F59E0B">{centerText("di-rac rea-dy")}</Text>
		</Box>
	);
};

/**
 * AsciiMotionCli - Now a static version of the Dirac logo.
 * Maintained for compatibility with existing views, but with all animation logic removed.
 */
export const AsciiMotionCli: React.FC<AsciiMotionCliProps> = ({ onReady, onInteraction }) => {
	useEffect(() => {
		if (onReady) {
			onReady({
				play: () => {},
				pause: () => {},
				restart: () => {},
			});
		}
	}, [onReady]);

	// Trigger onInteraction to allow dismissing the welcome state via any keypress
	useInput(() => {
		if (onInteraction) {
			onInteraction();
		}
	});

	return <StaticRobotFrame />;
};
