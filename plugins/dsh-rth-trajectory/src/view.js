		//#region rth/RthTrajectoryView.js (RTH addition, see NOTICE.md)
		/**
		* The RTH trajectory view: upstream's Toolbar, Overview, Table and inspector over RTH's
		* Mind projection (src/adapter.mjs), with kind filter chips and a session strip.
		* Upstream TrajectoryView above is kept unchanged and unused.
		*/
		const RTH_DEFAULT_GROUPS = () => new Set(RTH.DEFAULT_GROUPS);
		function RthTrajectoryView({ useRth, useDuration, loadImage, setActualDuration, viewRequest, completeViewRequest, renderSlot, t, jsonStringWrapping }) {
			RTH.ensureStyle();
			const data = useRth((snapshot) => snapshot);
			const [enabled, setEnabled] = (0, react.useState)(RTH_DEFAULT_GROUPS);
			const [collapsedTurns, setCollapsedTurns] = (0, react.useState)(EMPTY_TURN_IDS);
			const [collapsedAssistants, setCollapsedAssistants] = (0, react.useState)(EMPTY_RECORD_IDS);
			const [timelineSelection, setTimelineSelection] = (0, react.useState)(null);
			const [searchQuery, setSearchQuery] = (0, react.useState)("");
			const [searchIndex] = (0, react.useState)(() => new TrajectorySearchIndex());
			const [searchIndexRevision, setSearchIndexRevision] = (0, react.useState)(0);
			// Selection is held by record identity: display indexes are renumbered on every rebuild.
			const [selectedRecordId, setSelectedRecordId] = (0, react.useState)(null);
			const [timelineRecordSelection, setTimelineRecordSelection] = (0, react.useState)(null);
			const [timelineRecordFocus, setTimelineRecordFocus] = (0, react.useState)(null);
			const [live, setLive] = (0, react.useState)(true);
			RTH.setCards(data.cards);
			const timelineTurns = (0, react.useMemo)(() => RTH.filterTurns(data.turns, enabled), [data.turns, enabled]);
			const searchLayouts = (0, react.useMemo)(() => [timelineTurns], [timelineTurns]);
			const recordAt = (0, react.useMemo)(() => {
				const byIndex = /* @__PURE__ */ new Map();
				const byId = /* @__PURE__ */ new Map();
				for (const turn of timelineTurns) for (const group of turn.groups) for (const cell of group.cells) {
					byIndex.set(cell.index, cell);
					byId.set(trajectoryRecordId(cell), cell);
				}
				return { byIndex, byId };
			}, [timelineTurns]);
			const selectedTimelineIndex = selectedRecordId === null ? null : recordAt.byId.get(selectedRecordId)?.index ?? null;
			const setSelectedTimelineIndex = (0, react.useCallback)((index) => {
				const cell = index === null ? void 0 : recordAt.byIndex.get(index);
				setSelectedRecordId(cell === void 0 ? null : trajectoryRecordId(cell));
			}, [recordAt]);
			// Live: follow the newest ARRIVAL (the stream's arrival numbers, not display
			// positions) until the reader selects, moves or scrolls away; "Paused · N new" counts the source
			// rows that arrived since, by identity, so eviction, folding and filtering do not distort it.
			const latestArrival = data.latestArrival ?? 0;
			(0, react.useEffect)(() => {
				if (!live) return;
				let newest = null;
				for (const turn of timelineTurns) for (const group of turn.groups) for (const cell of group.cells) if (newest === null || (cell.rthArrival ?? -1) > (newest.rthArrival ?? -1) || (cell.rthArrival ?? -1) === (newest.rthArrival ?? -1) && cell.index > newest.index) newest = cell;
				if (newest !== null) setTimelineRecordFocus({ index: newest.index });
			}, [live, latestArrival, enabled]);
			const pausedAt = (0, react.useRef)(null);
			if (live) pausedAt.current = null;
			else if (pausedAt.current === null) pausedAt.current = latestArrival;
			const newSincePause = live ? 0 : RTH.newSince(timelineTurns, pausedAt.current, data.arrivals);
			// Scrolling away from the tail pauses Live; scrolling back to it resumes. Only scrolls the reader
			// caused (wheel, touch, keys, scrollbar drag) count, so the view's own focus scrolls never do.
			const rootRef = (0, react.useRef)(null);
			(0, react.useEffect)(() => {
				const root = rootRef.current;
				if (root === null) return;
				let intent = 0;
				const mark = () => {
					intent = performance.now();
				};
				const onKey = (event) => {
					if ([
						"PageUp",
						"PageDown",
						"ArrowUp",
						"ArrowDown",
						"Home",
						"End",
						" "
					].includes(event.key)) mark();
				};
				const onScroll = (event) => {
					const el = event.target;
					if (!(el instanceof Element) || el.closest("[data-rth=trajectory]") !== root || el.scrollHeight <= el.clientHeight) return;
					if (performance.now() - intent > 800) return;
					const gap = el.scrollHeight - el.scrollTop - el.clientHeight;
					if (gap > 48) setLive(false);
					else if (gap <= 4) setLive(true);
				};
				root.addEventListener("wheel", mark, { passive: true });
				root.addEventListener("touchmove", mark, { passive: true });
				root.addEventListener("pointerdown", mark);
				root.addEventListener("keydown", onKey);
				root.addEventListener("scroll", onScroll, true);
				return () => {
					root.removeEventListener("wheel", mark);
					root.removeEventListener("touchmove", mark);
					root.removeEventListener("pointerdown", mark);
					root.removeEventListener("keydown", onKey);
					root.removeEventListener("scroll", onScroll, true);
				};
			}, [data.notRth]);
			// Decorative motion stops while the page is hidden.
			const [hidden, setHidden] = (0, react.useState)(() => typeof document !== "undefined" && document.visibilityState === "hidden");
			(0, react.useEffect)(() => {
				const onVisibility = () => setHidden(document.visibilityState === "hidden");
				document.addEventListener("visibilitychange", onVisibility);
				return () => document.removeEventListener("visibilitychange", onVisibility);
			}, []);
			// Keys: j/k move between records, Enter inspects (upstream row), / search, f filters, l live.
			const onRthKeyDown = (event) => {
				if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return;
				const target = event.target;
				if (target instanceof Element && target.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])") !== null) return;
				const root = event.currentTarget;
				switch (event.key) {
					case "j":
					case "k": {
						const rows = [...root.querySelectorAll("tr[data-record-index]")];
						if (rows.length === 0) return;
						const at = rows.indexOf(document.activeElement?.closest?.("tr[data-record-index]") ?? null);
						const step = event.key === "j" ? 1 : -1;
						const next = rows[at < 0 ? (step > 0 ? 0 : rows.length - 1) : Math.max(0, Math.min(rows.length - 1, at + step))];
						next.focus();
						next.scrollIntoView({ block: "nearest" });
						setLive(false);
						break;
					}
					case "/":
						root.querySelector("input[type=search]")?.focus();
						break;
					case "f":
						root.querySelector("[data-rth=trajectory-strip] .rth-chip")?.focus();
						break;
					case "l":
						setLive((current) => !current);
						break;
					default: return;
				}
				event.preventDefault();
			};
			(0, react.useEffect)(() => {
				if (searchIndex.update(searchLayouts)) setSearchIndexRevision((revision) => revision + 1);
			}, [searchIndex, searchLayouts]);
			// One time base per axis. Two overviews: Sequence (upstream's timeline, observation ORDER
			// only) and Duration (RTH.ClockOverview, producer-clock lane groups, one axis per clock
			// and lifetime). Upstream's timed modes are never used: they would place records at
			// observed_at + a producer duration.
			const timelineMode = "sequence";
			const actualDuration = useDuration((value) => value) === true;
			const clockLayout = (0, react.useMemo)(() => actualDuration ? RTH.layoutClockOverview(timelineTurns) : null, [actualDuration, timelineTurns]);
			const selectFromOverview = (index) => {
				setLive(false);
				setTimelineSelection(null);
				setTimelineRecordSelection({ index });
				setSelectedTimelineIndex(index);
			};
			const searchMatchRecordIds = (0, react.useMemo)(() => searchIndex.search(searchQuery), [searchIndex, searchIndexRevision, searchQuery]);
			const searchMatchIndexes = (0, react.useMemo)(() => {
				if (searchMatchRecordIds === null) return null;
				const indexes = /* @__PURE__ */ new Set();
				for (const turn of timelineTurns) for (const group of turn.groups) for (const cell of group.cells) if (searchMatchRecordIds.has(trajectoryRecordId(cell))) indexes.add(cell.index);
				return indexes;
			}, [timelineTurns, searchMatchRecordIds]);
			const timelineFocusIndexes = (0, react.useMemo)(() => timelineSelection === null ? null : trajectoryTimelineFocusIndexes(timelineTurns, timelineSelection, timelineMode), [timelineMode, timelineSelection, timelineTurns]);
			const handleRecordSelect = (0, react.useCallback)((index) => {
				setLive(false);
				if (timelineFocusIndexes !== null && !timelineFocusIndexes.has(index)) setTimelineSelection(null);
			}, [timelineFocusIndexes]);
			const collapsibleTurnIds = (0, react.useMemo)(() => timelineTurns.filter((turn) => turn.turn !== null && turn.groups.reduce((count, group) => count + group.cells.length, 0) > 1).flatMap((turn) => turn.turn === null ? [] : [turn.turn]), [timelineTurns]);
			const allTurnsCollapsed = collapsibleTurnIds.length > 0 && collapsibleTurnIds.every((turn) => collapsedTurns.has(turn));
			const toggleTurn = (turn) => {
				setCollapsedTurns((current) => {
					const collapsed = new Set(current);
					if (collapsed.has(turn)) collapsed.delete(turn);
					else collapsed.add(turn);
					return collapsed;
				});
			};
			const toggleAllTurns = () => {
				setCollapsedTurns((current) => {
					const collapsed = new Set(current);
					if (allTurnsCollapsed) for (const turn of collapsibleTurnIds) collapsed.delete(turn);
					else for (const turn of collapsibleTurnIds) collapsed.add(turn);
					return collapsed;
				});
			};
			const toggleGroup = (id) => {
				setEnabled((current) => {
					const next = new Set(current);
					if (next.has(id)) next.delete(id);
					else next.add(id);
					return next.size === 0 ? RTH_DEFAULT_GROUPS() : next;
				});
				setTimelineSelection(null);
			};
			if (data.notRth === true) return (0, react_jsx_runtime.jsx)("div", {
				className: `${views_module_css_default.root} rth-trajectory`,
				"data-rth": "trajectory",
				children: (0, react_jsx_runtime.jsx)("div", {
					className: "rth-notrth",
					children: t("rth.notRthSession")
				})
			});
			return (0, react_jsx_runtime.jsxs)("div", {
				className: `${views_module_css_default.root} rth-trajectory`,
				"data-rth": "trajectory",
				"data-rth-live": live ? "on" : "off",
				"data-hidden": hidden ? "" : void 0,
				ref: rootRef,
				"data-conversation-composer-overlay": "",
				tabIndex: -1,
				onKeyDown: onRthKeyDown,
				children: [
					(0, react_jsx_runtime.jsx)(TrajectoryToolbar, {
						rthSequenceOnly: false,
						actualDuration,
						onActualDurationChange: (next) => {
							setActualDuration(next);
							setTimelineSelection(null);
						},
						actualTime: false,
						onActualTimeChange: () => {},
						allTurnsCollapsed,
						onToggleAllTurns: toggleAllTurns,
						allAssistantsCollapsed: false,
						onToggleAllAssistants: () => {},
						searchQuery,
						onSearchQueryChange: setSearchQuery,
						t
					}),
					(0, react_jsx_runtime.jsx)(RTH.StripRow, {
						data,
						enabled,
						onToggle: toggleGroup,
						live,
						newSincePause,
						onLive: () => setLive((current) => !current)
					}),
					actualDuration ? (0, react_jsx_runtime.jsx)(RTH.ClockOverview, {
						layout: clockLayout,
						selectedIndex: selectedTimelineIndex,
						searchMatchIndexes,
						onSelect: selectFromOverview
					}) : (0, react_jsx_runtime.jsx)(TrajectoryTimeline, {
						t,
						turns: timelineTurns,
						mode: timelineMode,
						range: timelineSelection,
						hasEarlierRecords: false,
						onLoadEarlier: void 0,
						selectedIndex: selectedTimelineIndex,
						searchMatchIndexes,
						onRangeChange: setTimelineSelection,
						onRecordSelect: selectFromOverview,
						onRecordFocus: (index) => {
							setTimelineRecordFocus({ index });
						}
					}),
					(0, react_jsx_runtime.jsx)("div", {
						className: views_module_css_default.ledger,
						children: (0, react_jsx_runtime.jsx)(TrajectoryTable, {
							t,
							stringWrapping: jsonStringWrapping === void 0 ? void 0 : {
								...jsonStringWrapping,
								label: t("record.wrapLines")
							},
							renderSlot,
							loadImage,
							requestNumbers: [],
							turns: timelineTurns,
							streamingCells: [],
							timelineFocusIndexes,
							searchMatchIndexes,
							onSelectedIndexChange: setSelectedTimelineIndex,
							onRecordSelect: handleRecordSelect,
							recordSelection: timelineRecordSelection,
							recordFocus: timelineRecordFocus,
							historyLoading: data.stream === "opening",
							olderHistoryLoading: false,
							historyStartSeq: 0,
							hasOlderRecords: false,
							onLoadOlder: async () => false,
							onClearSelection: () => {
								setTimelineSelection(null);
							},
							collapsedTurns,
							onToggleTurn: toggleTurn,
							collapsedAssistants,
							onToggleAssistant: (id) => {
								setCollapsedAssistants((current) => {
									const collapsed = new Set(current);
									if (collapsed.has(id)) collapsed.delete(id);
									else collapsed.add(id);
									return collapsed;
								});
							},
							inspectCallId: null,
							onInspectApplied: completeViewRequest
						})
					})
				]
			});
		}
		//#endregion
