import org.bukkit.command.*;
import org.bukkit.event.*;
import org.bukkit.event.block.BlockPhysicsEvent;
import org.bukkit.plugin.java.JavaPlugin;

/** Integration-only: proves the engine guard preserves registered plugin listeners. */
public final class PhysicsListenerProbe extends JavaPlugin implements Listener {
    private long count;
    private boolean cancel;
    @EventHandler public void physics(BlockPhysicsEvent event) { count++; if (cancel) event.setCancelled(true); }
    @Override public boolean onCommand(CommandSender sender, Command command, String label, String[] args) {
        if (args.length != 1) return false;
        if (args[0].equals("count")) { sender.sendMessage("PHYSICS_COUNT " + count); return true; }
        HandlerList.unregisterAll((Listener)this);
        cancel = args[0].equals("cancel"); count = 0;
        if (!args[0].equals("off")) getServer().getPluginManager().registerEvents(this, this);
        sender.sendMessage("PHYSICS_MODE " + args[0]); return true;
    }
}
