		//#region rth/index.js (RTH replacement of lib/types/client/index.js, see NOTICE.md)
		/** Required services: the conversation slot and the locale service. No upstream Definitions, no Session paging. */
		const inject = [
			"slots",
			"uiConversation",
			"sidebarRightTabs",
			"sidebarRight",
			"locale"
		];
		const RTH_TAB_ID = "rth-trajectory-tab";
		const RTH_TAB_KIND = "rth.trajectory";
		/**
		* Client plugin body: register the RTH trajectory as its own conversation view ("rth-trajectory",
		* labelled "RTH"), next to Chat and upstream Trajectory, which stays untouched. RTH's feed is
		* shown only in the RTH session (rth-root); any other session gets an explanatory empty state.
		* The registration rides the slot service's effect wrapper, so plugin unload removes the tab.
		* @param ctx - client root context.
		*/
		function apply(ctx) {
			ctx.effect(() => ctx.locale.register(NS, {
				zh,
				en
			}), "rth-trajectory: dictionaries");
			const t = ctx.locale.bind(NS);
			const duration = createTrajectoryDurationStore();
			const stringWrapping = createTrajectoryStringWrappingStore();
			const viewInject = (sessionId) => ({
				hooks: {
					duration,
					rth: RTH.sourceFor(sessionId)
				},
				jsonStringWrapping: {
					getDefault: () => stringWrapping.getSnapshot(),
					setDefault: (value) => {
						stringWrapping.set(value);
					}
				},
				loadImage: Object.assign(async () => null, { peek: () => void 0 }),
				setActualDuration: (value) => {
					duration.set(value);
				}
			});
			// Placement 2 (the Bot profile): DSH's right sidebar, where the Bot keeps its tab group;
			// its Fullscreen control gives the ledger the full width. Same view, same shared stream.
			ctx.effect(() => ctx.sidebarRightTabs.register({
				id: RTH_TAB_ID,
				kind: RTH_TAB_KIND,
				priority: "extension",
				title: () => t("rth.tab"),
				guide: [{
					id: "rth-trajectory",
					order: 91,
					title: () => t("rth.tab"),
					description: () => t("rth.tabGuide")
				}]
			}), "rth-trajectory: sidebar tab type");
			ctx.effect(() => () => {
				for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
					if (tab.kind !== RTH_TAB_KIND) continue;
					try {
						ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId);
					} catch {}
				}
			}, "rth-trajectory: close open tabs on unload");
			ctx.slots.inject("sidebar.right.pane.tab", () => ctx.slots.register({
				name: "sidebar.right.pane.tab",
				key: RTH_TAB_ID,
				locale: NS,
				inject: viewInject
			}, RthTrajectoryView));
			ctx.slots.inject("sidebar.right.pane.tab.title", () => ctx.slots.register({
				name: "sidebar.right.pane.tab.title",
				key: RTH_TAB_ID,
				locale: NS
			}, () => t("rth.tab")));
			// Placement 1 (plain DSH): its own conversation view, next to Chat and upstream Trajectory.
			ctx.slots.inject("conversation.view", () => ctx.slots.register({
				name: "conversation.view",
				id: "rth-trajectory",
				order: 5,
				locale: NS,
				label: () => t("rth.view"),
				inject: viewInject
			}, RthTrajectoryView));
			// The Bot profile hides DSH's view tabs; its chat header takes a button instead (RTH Main Bot only).
			let omni = null;
			try {
				omni = require("@automagik/omni-dsh/client").omni ?? null;
			} catch {}
			if (omni?.headerButtons !== void 0) ctx.effect(() => omni.headerButtons.register({
				id: "rth-trajectory",
				order: 50,
				Component: RTH.makeHeaderButton(() => {
					ctx.sidebarRight.openTabIn(RTH.RTH_SESSION, RTH_TAB_KIND);
					if (ctx.sidebarRight.isExpanded?.() === false) ctx.sidebarRight.toggleExpanded?.();
				})
			}), "rth-trajectory: Bot header button");
		}
		//#endregion
