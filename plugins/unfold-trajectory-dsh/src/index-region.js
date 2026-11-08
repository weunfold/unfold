		//#region unfold/index.js (Unfold replacement of lib/types/client/index.js, see NOTICE.md)
		/** Required services: the conversation slot and the locale service. No upstream Definitions, no Session paging. */
		const inject = [
			"slots",
			"uiConversation",
			"sidebarRightTabs",
			"sidebarRight",
			"locale"
		];
		const UNFOLD_TAB_ID = "unfold-trajectory-tab";
		const UNFOLD_TAB_KIND = "unfold.trajectory";
		/**
		* Client plugin body: register the Unfold trajectory as its own conversation view ("unfold-trajectory",
		* labelled "Unfold"), next to Chat and upstream Trajectory, which stays untouched. Unfold's feed is
		* shown only in the Unfold session (unfold-root); any other session gets an explanatory empty state.
		* The registration rides the slot service's effect wrapper, so plugin unload removes the tab.
		* @param ctx - client root context.
		*/
		async function apply(ctx) {
			await UNFOLD.discover();
			ctx.effect(() => ctx.locale.register(NS, {
				zh,
				en
			}), "unfold-trajectory: dictionaries");
			const t = ctx.locale.bind(NS);
			const duration = createTrajectoryDurationStore();
			const stringWrapping = createTrajectoryStringWrappingStore();
			const viewInject = (sessionId) => ({
				hooks: {
					duration,
					unfold: UNFOLD.sourceFor(sessionId)
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
				id: UNFOLD_TAB_ID,
				kind: UNFOLD_TAB_KIND,
				priority: "extension",
				title: () => t("unfold.tab"),
				guide: [{
					id: "unfold-trajectory",
					order: 91,
					title: () => t("unfold.tab"),
					description: () => t("unfold.tabGuide")
				}]
			}), "unfold-trajectory: sidebar tab type");
			ctx.effect(() => () => {
				for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
					if (tab.kind !== UNFOLD_TAB_KIND) continue;
					try {
						ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId);
					} catch {}
				}
			}, "unfold-trajectory: close open tabs on unload");
			ctx.slots.inject("sidebar.right.pane.tab", () => ctx.slots.register({
				name: "sidebar.right.pane.tab",
				key: UNFOLD_TAB_ID,
				locale: NS,
				inject: viewInject
			}, UnfoldTrajectoryView));
			ctx.slots.inject("sidebar.right.pane.tab.title", () => ctx.slots.register({
				name: "sidebar.right.pane.tab.title",
				key: UNFOLD_TAB_ID,
				locale: NS
			}, () => t("unfold.tab")));
			// Placement 1 (plain DSH): its own conversation view, next to Chat and upstream Trajectory.
			ctx.slots.inject("conversation.view", () => ctx.slots.register({
				name: "conversation.view",
				id: "unfold-trajectory",
				order: 5,
				locale: NS,
				label: () => t("unfold.view"),
				inject: viewInject
			}, UnfoldTrajectoryView));
			// The Bot profile hides DSH's view tabs; its chat header takes a button instead (Unfold Main Bot only).
			let omni = null;
			try {
				omni = require("@automagik/bots-dsh/client").omni ?? null;
			} catch {}
			if (omni?.headerButtons !== void 0) ctx.effect(() => omni.headerButtons.register({
				id: "unfold-trajectory",
				order: 50,
				Component: UNFOLD.makeHeaderButton(() => {
					ctx.sidebarRight.openTabIn(UNFOLD.UNFOLD_SESSION, UNFOLD_TAB_KIND);
					if (ctx.sidebarRight.isExpanded?.() === false) ctx.sidebarRight.toggleExpanded?.();
				})
			}), "unfold-trajectory: Bot header button");
		}
		//#endregion
